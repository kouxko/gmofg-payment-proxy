#!/usr/bin/env ruby
# frozen_string_literal: true

# Android 模拟器上的联合门禁：让被定向接管的 shell UID 在固定延迟生效时，分别访问
# DLL 与 Transaction 两个 Reverse Listener。这里只验证“VPN 数据面 + 通用代理 + D48
# 字节透传”的组合链路；真实 GMO-FG 业务验收仍必须使用 A920MAX 和真实上游。

require "json"
require "open3"
require "socket"
require "time"
require "timeout"
require_relative "../android-control-client"

serial = ENV.fetch("ANDROID_SERIAL")
host_port = Integer(ENV.fetch("EMULATOR_PROXY_GATE_HOST_PORT"))
report_file = ENV.fetch("EMULATOR_PROXY_GATE_VPN_REPORT_FILE")
host = ENV.fetch("EMULATOR_PROXY_GATE_ANDROID_HOST", "10.0.3.2")
relay_dll_port = host_port + 2_000
relay_transaction_port = host_port + 2_001
target_package = ENV.fetch("EMULATOR_PROXY_GATE_TARGET_PACKAGE")
target_activity = ENV.fetch("EMULATOR_PROXY_GATE_TARGET_ACTIVITY")
target_uid = Integer(ENV.fetch("EMULATOR_PROXY_GATE_TARGET_UID"))
target_signature = ENV.fetch("EMULATOR_PROXY_GATE_TARGET_SIGNING_SHA256")

def run(*command, allow_failure: false)
  stdout, stderr, status = Open3.capture3(*command)
  return [stdout, stderr, status] if allow_failure || status.success?

  raise "command failed (#{status.exitstatus}): #{command.join(' ')}\n#{stdout}\n#{stderr}"
end

def control(serial, operation, profile = nil)
  AndroidControlClient.request(serial: serial, operation: operation, profile: profile)
end

def wait_for_vpn_running(serial, timeout_seconds: 8)
  deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + timeout_seconds
  loop do
    status = control(serial, "status")
    return status if status.fetch("state") == "running"

    if %w[faulted stopped].include?(status.fetch("state"))
      raise "VPN did not enter running state: #{status}"
    end
    raise "VPN start timed out: #{status}" if Process.clock_gettime(Process::CLOCK_MONOTONIC) >= deadline

    sleep 0.1
  end
end

def request_through_vpn(serial, package_name, activity_name, host, port, path)
  body = '{"operation":"probe"}'
  token = Process.clock_gettime(Process::CLOCK_MONOTONIC, :millisecond).to_i.to_s
  file_name = "probe-#{token}.txt"
  run("adb", "-s", serial, "shell", "run-as", package_name, "rm", "-f", "files/#{file_name}", allow_failure: true)
  started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  _, start_stderr, start_status = run(
    "adb", "-s", serial, "shell", "am", "start", "-W", "-n",
    "#{package_name}/#{activity_name}", "--es", "host", host, "--ei", "port", port.to_s,
    "--ei", "timeout", "7000", "--es", "token", token, "--es", "path", path,
    "--es", "method", "POST", "--es", "body", body,
    allow_failure: true
  )
  stdout = ""
  stderr = start_stderr
  exit_status = start_status.exitstatus
  deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + 9
  loop do
    stdout, read_stderr, read_status = run(
      "adb", "-s", serial, "shell", "run-as", package_name, "cat", "files/#{file_name}",
      allow_failure: true
    )
    if read_status.success?
      stderr = read_stderr
      exit_status = stdout.start_with?("ERROR:") ? 1 : 0
      break
    end
    if Process.clock_gettime(Process::CLOCK_MONOTONIC) >= deadline
      stderr = "#{stderr}\n#{read_stderr}\n应用进程网络探针超时或未生成结果"
      exit_status = 1
      break
    end
    sleep 0.1
  end
  {
    stdout: stdout,
    stderr: stderr,
    exit: exit_status,
    elapsed_millis: ((Process.clock_gettime(Process::CLOCK_MONOTONIC) - started) * 1000).round
  }
end

def request_direct(serial, host, port, path)
  body = '{"operation":"probe"}'
  request = "POST #{path} HTTP/1.1\r\nHost: #{host}:#{port}\r\nContent-Type: application/json; charset=UTF-8\r\nContent-Length: #{body.bytesize}\r\nConnection: close\r\n\r\n#{body}"
  encoded_request = [request].pack("m0")
  command = "timeout 10 sh -c \"{ printf '%s' '#{encoded_request}' | base64 -d; sleep 3; } | nc -w 7 -W 7 #{host} #{port}\""
  started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  stdout, stderr, status = run("adb", "-s", serial, "shell", command, allow_failure: true)
  {
    stdout: stdout,
    stderr: stderr,
    exit: status.exitstatus,
    elapsed_millis: ((Process.clock_gettime(Process::CLOCK_MONOTONIC) - started) * 1000).round
  }
end

def response_evidence(result)
  raw = result.fetch(:stdout).dup.force_encoding(Encoding::BINARY)
  separator = raw.index("\r\n\r\n".b)
  head = separator.nil? ? raw : raw.byteslice(0, separator)
  body = separator.nil? ? "".b : raw.byteslice(separator + 4..)
  {
    exit: result.fetch(:exit),
    elapsed_millis: result.fetch(:elapsed_millis),
    response_bytes: raw.bytesize,
    response_head: head.dup.force_encoding(Encoding::ISO_8859_1).encode(Encoding::UTF_8),
    body_shift_jis: body.dup.force_encoding("Shift_JIS").encode(
      Encoding::UTF_8,
      invalid: :replace,
      undef: :replace
    ),
    body_hex: body.unpack1("H*"),
    stderr: result.fetch(:stderr).encode(Encoding::UTF_8, invalid: :replace, undef: :replace)
  }
end

def start_transparent_relay(label, listen_port, target_port)
  server = TCPServer.new("0.0.0.0", listen_port)
  thread = Thread.new do
    loop do
      client = server.accept
      Thread.new(client) do |downstream|
        upstream = TCPSocket.new("127.0.0.1", target_port)
        upload = Thread.new do
          # 必须边读边转发，不能等设备端 EOF 后再一次性写入。HTTP 客户端会保持写方向
          # 打开并等待响应；若 relay 等 EOF，客户端与代理就会互相等待。
          uploaded = IO.copy_stream(downstream, upstream)
          warn "#{label} relay uploaded #{uploaded} bytes"
          upstream.close_write
        rescue IOError, SystemCallError => error
          warn "#{label} upload relay stopped: #{error.class}: #{error.message}"
        end
        download = Thread.new do
          downloaded = IO.copy_stream(upstream, downstream)
          warn "#{label} relay downloaded #{downloaded} bytes"
          downstream.close_write
        rescue IOError, SystemCallError => error
          warn "#{label} download relay stopped: #{error.class}: #{error.message}"
        end
        upload.join
        download.join
      ensure
        upstream&.close
        downstream.close
      end
    end
  rescue IOError, SystemCallError => error
    warn "#{label} accept relay stopped: #{error.class}: #{error.message}"
  end
  [server, thread]
end

weak_network = {
  seed: 48,
  fixed_delay_millis: 120,
  uniform_jitter_millis: 30,
  upload_bytes_per_second: nil,
  download_bytes_per_second: nil,
  random_loss_basis_points: 0,
  burst_loss: nil,
  duplicate_basis_points: 0,
  reorder_basis_points: 0,
  maximum_reorder_hold_millis: 0,
  blackout_windows: [],
  dns_blackhole: false,
  nth_tcp_flag_drops: [],
  path_mtu: { mtu: nil, mss_clamp: nil, mode: "pass" },
  corruption: { probability_basis_points: 0, bits_per_packet: 0 }
}
profile = {
  id: "proxy-vpn-joint-d48",
  name: "Proxy VPN joint D48 gate",
  target_applications: [{
    package_name: target_package,
    signing_sha256: target_signature,
    uid: target_uid
  }],
  destination_targets: [{ cidr: host, ports: [relay_dll_port, relay_transaction_port] }],
  confirmed_shared_uids: [],
  auto_resume_after_reboot: false,
  weak_network: weak_network
}

report = {
  scope: "TEST ONLY Android emulator VPN plus simulated DLL and Transaction upstreams",
  serial: serial,
  target_package: target_package,
  destination: host,
  ports: {
    dll_listener: host_port,
    transaction_listener: host_port + 1,
    dll_relay: relay_dll_port,
    transaction_relay: relay_transaction_port
  },
  started_at: Time.now.iso8601
}

begin
  dll_relay, dll_relay_thread = start_transparent_relay("DLL", relay_dll_port, host_port)
  transaction_relay, transaction_relay_thread = start_transparent_relay(
    "Transaction",
    relay_transaction_port,
    host_port + 1
  )
  # run.sh 已通过 appops 完成模拟器测试授权。这里不能再次打开授权 Activity：已授权时
  # Activity 会自动启动磁盘中上次保存的 Profile，与随后 apply 的本轮 Profile 并发。
  control(serial, "emergency_restore")
  sleep 1

  # 基线使用未被 Profile 选中的 shell UID。若先让目标 Probe 建立直连，再为同一 UID 建立
  # TUN，旧 TCP 流的收尾包也会进入新 TUN，制造与本轮请求无关的 SOCKS 重连噪声。
  direct_dll = request_direct(serial, host, relay_dll_port, "/direct/dll")
  direct_transaction = request_direct(
    serial,
    host,
    relay_transaction_port,
    "/direct/transaction"
  )
  unless direct_dll[:stdout].include?("HTTP/1.1 200") && direct_dll[:stdout].include?("D48")
    raise "direct DLL baseline did not return D48: #{direct_dll}"
  end
  unless direct_transaction[:stdout].include?("HTTP/1.1 200") && direct_transaction[:stdout].include?("D48")
    raise "direct Transaction baseline did not return D48: #{direct_transaction}"
  end

  control(serial, "apply", profile)
  # apply 只确认 startForegroundService 请求已经发出；VpnService 建立 TUN 和启动 Rust
  # 数据面是异步的。必须等到 Companion 明确报告 running，避免首个请求绕过尚未生效的
  # allowlist，同时旧连接的 SYN 又在 TUN 建立后被重复接管。
  report[:vpn_status_after_apply] = wait_for_vpn_running(serial)

  dll = request_through_vpn(
    serial,
    target_package,
    target_activity,
    host,
    relay_dll_port,
    "/vpn/dll"
  )
  report[:vpn_status_after_dll] = control(serial, "status")
  transaction = request_through_vpn(
    serial,
    target_package,
    target_activity,
    host,
    relay_transaction_port,
    "/vpn/transaction"
  )
  status = control(serial, "status")
  runtime_stats = status.fetch("stats")
  # 即使后续严格门禁因为原生数据面错误而失败，也把当时的完整运行快照写入报告。
  # 这样可以区分“业务请求本身失败”和“同一运行周期存在额外 SOCKS 会话失败”。
  report[:vpn_status] = status
  delay_total = runtime_stats.fetch("impairment_delay_millis_total").to_i

  raise "DLL did not return HTTP 200 with D48: #{dll}" unless dll[:stdout].include?("HTTP/1.1 200") && dll[:stdout].include?("D48")
  unless transaction[:stdout].include?("HTTP/1.1 200") && transaction[:stdout].include?("D48")
    raise "Transaction did not return HTTP 200 with D48: #{transaction}"
  end
  raise "VPN delay was not observed in Rust counters: #{status}" unless delay_total.positive?
  unless runtime_stats["last_error"].nil? || runtime_stats["last_error"].to_s.empty?
    raise "VPN data plane reported an unexpected error: #{runtime_stats.fetch('last_error')}"
  end

  report[:result] = "PASS"
  report[:direct_dll] = response_evidence(direct_dll)
  report[:direct_transaction] = response_evidence(direct_transaction)
  report[:dll] = response_evidence(dll)
  report[:transaction] = response_evidence(transaction)
rescue StandardError => error
  report[:result] = "FAIL"
  report[:error] = "#{error.class}: #{error.message}".encode(
    Encoding::UTF_8,
    invalid: :replace,
    undef: :replace
  )
ensure
  control(serial, "emergency_restore") rescue nil
  dll_relay&.close
  transaction_relay&.close
  dll_relay_thread&.join(1)
  transaction_relay_thread&.join(1)
  report[:finished_at] = Time.now.iso8601
  File.write(report_file, JSON.pretty_generate(report))
end

puts JSON.pretty_generate(report)
exit(report[:result] == "PASS" ? 0 : 1)
