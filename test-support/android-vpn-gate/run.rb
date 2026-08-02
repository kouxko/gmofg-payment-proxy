#!/usr/bin/env ruby
# frozen_string_literal: true

require "json"
require "open3"
require "socket"
require "time"
require "timeout"
require_relative "../android-control-client"

SERIAL = ENV.fetch("ANDROID_SERIAL", "127.0.0.1:6555")
TARGET_PACKAGE = "com.interceptproxy.vpn.targetprobe"
TARGET_UID = Integer(ENV.fetch("ANDROID_VPN_GATE_TARGET_UID"))
NON_TARGET_PACKAGES = [
  "com.interceptproxy.vpn",
  "com.interceptproxy.vpn.isolationprobe"
].freeze
TARGET_SIGNING_SHA256 = ENV.fetch("ANDROID_VPN_GATE_TARGET_SIGNING_SHA256")
HOST = "10.0.3.2"
TCP_PORT = 18_080
UDP_PORT = 18_081
IPV6_HOST = ENV["ANDROID_VPN_GATE_IPV6_HOST"]&.strip
IPV6_TCP_PORT = Integer(ENV.fetch("ANDROID_VPN_GATE_IPV6_TCP_PORT", "18082"))
IPV6_UDP_PORT = Integer(ENV.fetch("ANDROID_VPN_GATE_IPV6_UDP_PORT", "18083"))
STOP_RECOVERY_DEADLINE_SECONDS = 5

def run(*command, allow_failure: false)
  stdout, stderr, status = Open3.capture3(*command)
  return [stdout, stderr, status] if allow_failure || status.success?

  raise "command failed (#{status.exitstatus}): #{command.join(' ')}\n#{stdout}\n#{stderr}"
end

def adb_shell(command, allow_failure: false)
  run("adb", "-s", SERIAL, "shell", command, allow_failure: allow_failure)
end

def control(operation, profile = nil)
  AndroidControlClient.request(serial: SERIAL, operation: operation, profile: profile)
end

def zero_weak_network
  {
    seed: 1,
    fixed_delay_millis: 0,
    uniform_jitter_millis: 0,
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
end

def profile(name, overrides = nil, destinations: [], **override_keywords)
  # Ruby 3 不再把关键字参数自动转换为位置 Hash。门禁既有
  # `profile("delay", fixed_delay_millis: 100)`，也有需要同时传目标地址数组的
  # `profile("target", { ... }, destinations: [...])`；这里显式合并两种写法。
  weak = zero_weak_network.merge(overrides || {}).merge(override_keywords)
  {
    id: "vpn-gate-#{name}",
    name: "VPN gate #{name}",
    target_applications: [{
      package_name: TARGET_PACKAGE,
      signing_sha256: TARGET_SIGNING_SHA256,
      uid: TARGET_UID
    }],
    destination_targets: destinations,
    confirmed_shared_uids: [],
    auto_resume_after_reboot: false,
    weak_network: weak
  }
end

def wait_running(expected_id)
  deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + 8
  loop do
    status = control("status")
    return status if status["state"] == "running" && status["active_profile_id"] == expected_id

    raise "VPN did not reach running state: #{status}" if Process.clock_gettime(Process::CLOCK_MONOTONIC) >= deadline

    sleep 0.15
  end
end

def apply_profile(value)
  control("apply", value)
  wait_running(value.fetch(:id))
end

def target_http(path: "/small", timeout: 8, host: HOST, port: TCP_PORT)
  started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  result = app_probe(TARGET_PACKAGE, timeout: timeout, host: host, port: port, path: path)
  elapsed = ((Process.clock_gettime(Process::CLOCK_MONOTONIC) - started) * 1000).round
  result.merge(elapsed_millis: elapsed)
end

def target_upload(bytes: 8_192, timeout: 20)
  started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  result = app_probe(
    TARGET_PACKAGE,
    timeout: timeout,
    path: "/upload",
    method: "POST",
    body_bytes: bytes
  )
  elapsed = ((Process.clock_gettime(Process::CLOCK_MONOTONIC) - started) * 1000).round
  result.merge(elapsed_millis: elapsed, bytes: bytes)
end

def package_uid(package_name)
  output = adb_shell("cmd package list packages -U #{package_name}").first
  Integer(output[/uid:(\d+)/, 1])
end

def non_target_http(package_name, timeout: 5)
  return app_probe(package_name, timeout: timeout) if package_name.end_with?(".isolationprobe")

  request = "GET /small HTTP/1.1\r\nHost: #{HOST}\r\nConnection: close\r\n\r\n"
  command = "run-as #{package_name} sh -c \"{ printf '#{request}'; sleep 1; } | nc -w #{timeout} -W #{timeout} #{HOST} #{TCP_PORT}\""
  stdout, stderr, status = adb_shell(command, allow_failure: true)
  { stdout: stdout, stderr: stderr, exit: status.exitstatus }
end

def app_probe(
  package_name,
  timeout:,
  host: HOST,
  path: "/small",
  method: "GET",
  body_bytes: 0,
  protocol: "tcp",
  port: TCP_PORT
)
  token = Process.clock_gettime(Process::CLOCK_MONOTONIC, :millisecond).to_i.to_s
  file_name = "probe-#{token}.txt"
  adb_shell("run-as #{package_name} rm -f files/#{file_name}", allow_failure: true)
  run(
    "adb", "-s", SERIAL, "shell", "am", "start", "-W", "-n",
    "#{package_name}/com.interceptproxy.vpn.isolationprobe.ProbeActivity",
    "--es", "host", host, "--ei", "port",
    port.to_s, "--ei", "timeout", (timeout * 1_000).to_s, "--es", "token", token,
    "--es", "path", path, "--es", "method", method, "--ei", "body_bytes", body_bytes.to_s,
    "--es", "protocol", protocol,
    allow_failure: true
  )
  deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + timeout + 2
  loop do
    stdout, stderr, status = adb_shell(
      "run-as #{package_name} cat files/#{file_name}",
      allow_failure: true
    )
    return { stdout: stdout, stderr: stderr, exit: 0 } if status.success?
    break if Process.clock_gettime(Process::CLOCK_MONOTONIC) >= deadline

    sleep 0.1
  end
  { stdout: "", stderr: "应用进程网络探针超时或未生成结果", exit: 1 }
end

def target_udp(timeout: 4, host: HOST, port: UDP_PORT)
  app_probe(TARGET_PACKAGE, timeout: timeout, host: host, protocol: "udp", port: port)
end

def stats
  control("status").fetch("stats")
end

def assert(label, condition, details = nil)
  raise "#{label} failed#{details.nil? ? '' : ": #{details}"}" unless condition
end

def packet_totals(value)
  %w[tun_upload_packets tun_download_packets proxy_upload_packets proxy_download_packets]
    .to_h { |key| [key, value[key].to_i] }
end

def ipv6_device_environment
  addresses = adb_shell("ip -6 addr show", allow_failure: true)
  routes = adb_shell("ip -6 route show", allow_failure: true)
  {
    configured_host: IPV6_HOST,
    addresses: addresses[0],
    address_error: addresses[1],
    routes: routes[0],
    route_error: routes[1]
  }
end

def stop_and_verify_recovery
  started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  stop_response = control("stop")
  attempts = []
  recovered = false
  loop do
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - started
    break if elapsed >= STOP_RECOVERY_DEADLINE_SECONDS

    probe = target_http(timeout: 1)
    attempts << probe.slice(:exit, :elapsed_millis, :stderr, :stdout)
    if probe[:stdout].include?("VPN-BASELINE")
      recovered = true
      break
    end
  end
  elapsed_millis = ((Process.clock_gettime(Process::CLOCK_MONOTONIC) - started) * 1000).round
  {
    status: recovered && elapsed_millis <= STOP_RECOVERY_DEADLINE_SECONDS * 1_000 ? "PASS" : "FAIL",
    recovered: recovered,
    elapsed_millis: elapsed_millis,
    deadline_millis: STOP_RECOVERY_DEADLINE_SECONDS * 1_000,
    stop_response: stop_response,
    attempts: attempts
  }
end

tcp_server = TCPServer.new("0.0.0.0", TCP_PORT)
udp_server = UDPSocket.new
udp_server.bind("0.0.0.0", UDP_PORT)
ipv6_tcp_server = IPV6_HOST.nil? || IPV6_HOST.empty? ? nil : TCPServer.new("::", IPV6_TCP_PORT)
ipv6_udp_server = if IPV6_HOST.nil? || IPV6_HOST.empty?
                    nil
                  else
                    UDPSocket.new(Socket::AF_INET6).tap { |socket| socket.bind("::", IPV6_UDP_PORT) }
                  end
servers = [
  Thread.new do
    begin
      loop do
        client = tcp_server.accept
        Thread.new(client) do |connection|
          head = +""
          head << connection.readpartial(1024) until head.include?("\r\n\r\n")
          header, buffered_body = head.split("\r\n\r\n", 2)
          path = head.lines.first.to_s.split[1]
          if path == "/upload"
            content_length = header[/^Content-Length:\s*(\d+)/i, 1].to_i
            received = buffered_body.to_s.bytesize
            received += connection.readpartial([content_length - received, 16_384].min).bytesize while received < content_length
            body = "VPN-UPLOAD-#{received}"
          else
            body = path == "/large" ? ("L" * 32_768) : "VPN-BASELINE"
          end
          connection.write("HTTP/1.1 200 OK\r\nContent-Length: #{body.bytesize}\r\nConnection: close\r\n\r\n#{body}")
        rescue EOFError, IOError
          nil
        ensure
          connection.close
        end
      end
    rescue IOError
      nil
    end
  end,
  Thread.new do
    begin
      loop do
        payload, sender = udp_server.recvfrom(2048)
        udp_server.send(payload, 0, sender[3], sender[1])
      end
    rescue IOError
      nil
    end
  end
]

if ipv6_tcp_server && ipv6_udp_server
  servers << Thread.new do
    begin
      loop do
        client = ipv6_tcp_server.accept
        Thread.new(client) do |connection|
          head = +""
          head << connection.readpartial(1024) until head.include?("\r\n\r\n")
          body = "VPN-BASELINE"
          connection.write("HTTP/1.1 200 OK\r\nContent-Length: #{body.bytesize}\r\nConnection: close\r\n\r\n#{body}")
        rescue EOFError, IOError
          nil
        ensure
          connection.close
        end
      end
    rescue IOError
      nil
    end
  end
  servers << Thread.new do
    begin
      loop do
        payload, sender = ipv6_udp_server.recvfrom(2048)
        ipv6_udp_server.send(payload, 0, sender[3], sender[1])
      end
    rescue IOError
      nil
    end
  end
end

report = {
  serial: SERIAL,
  target_package: TARGET_PACKAGE,
  scenarios: {},
  skips: [],
  supported_boundaries: {
    ipv6_extension_headers: {
      status: "LIMITED",
      behavior: "通用延迟、抖动、限速、随机丢包等整包故障仍生效；端口过滤、DNS 黑洞和第 N 个 TCP 标志规则不匹配。",
      reason: "Rust 数据面当前只解析 IPv6 基本头后直接承载的 TCP/UDP，扩展头报文按 Other 传输层处理。"
    }
  },
  started_at: Time.now.iso8601
}

begin
  run("adb", "-s", SERIAL, "get-state")
  # Android 会冻结后台缓存进程；仅连接 localabstract socket 不保证解冻。显式启动授权页
  # 只用于唤醒 Companion，不点击、不修改授权状态。
  run(
    "adb", "-s", SERIAL, "shell", "am", "start", "-n",
    "com.interceptproxy.vpn/.VpnConsentActivity",
    allow_failure: true
  )
  sleep 0.5
  # 清理上次异常中断遗留的 service action，等待系统彻底撤销旧 TUN，再开始基线。
  control("emergency_restore")
  sleep 1
  run(
    "adb", "-s", SERIAL, "shell", "am", "start", "-n",
    "com.interceptproxy.vpn/.VpnConsentActivity",
    allow_failure: true
  )
  sleep 0.5

  baseline = profile("baseline")
  report[:baseline_running_status] = apply_profile(baseline)
  tcp = target_http
  udp = target_udp
  non_target_uids = NON_TARGET_PACKAGES.to_h { |package_name| [package_name, package_uid(package_name)] }
  assert("non-target UIDs are distinct", non_target_uids.values.uniq.length == NON_TARGET_PACKAGES.length, non_target_uids)
  direct = NON_TARGET_PACKAGES.to_h { |package_name| [package_name, non_target_http(package_name)] }
  baseline_stats = stats
  report[:baseline_probe] = { tcp: tcp, udp: udp, non_target: direct, stats: baseline_stats }
  assert("baseline target TCP", tcp[:stdout].include?("VPN-BASELINE"), tcp)
  assert("baseline target UDP", udp[:stdout].include?("UDP-VPN-GATE"), udp)
  assert("baseline non-target bypass", direct.values.all? { |probe| probe[:stdout].include?("VPN-BASELINE") }, direct)
  assert("baseline SOCKS connect", baseline_stats["socks_connect_successes"].to_i.positive?, baseline_stats)
  report[:scenarios][:baseline_ipv4_tcp_udp_and_isolation] = { tcp: tcp, udp: udp, non_target_uids: non_target_uids, non_targets: direct, stats: baseline_stats }

  ipv6_environment = ipv6_device_environment
  report[:ipv6_environment] = ipv6_environment
  if IPV6_HOST.nil? || IPV6_HOST.empty?
    reason = "未配置 ANDROID_VPN_GATE_IPV6_HOST，且设备没有 IPv6 默认路由，无法建立可验证的宿主 IPv6 TCP/UDP 端点。"
    %i[ipv6_tcp_forwarding_and_isolation ipv6_udp_forwarding_and_isolation].each do |scenario|
      report[:scenarios][scenario] = { status: "SKIP", reason: reason, environment: ipv6_environment }
      report[:skips] << scenario
    end
  else
    apply_profile(profile("ipv6-baseline"))

    ipv6_tcp = target_http(host: IPV6_HOST, port: IPV6_TCP_PORT)
    ipv6_tcp_target_stats = stats
    ipv6_tcp_non_target = app_probe(
      "com.interceptproxy.vpn.isolationprobe",
      timeout: 5,
      host: IPV6_HOST,
      port: IPV6_TCP_PORT
    )
    ipv6_tcp_after_non_target = stats
    assert("IPv6 target TCP forwarding", ipv6_tcp[:stdout].include?("VPN-BASELINE"), ipv6_tcp)
    assert("IPv6 non-target TCP bypass", ipv6_tcp_non_target[:stdout].include?("VPN-BASELINE"), ipv6_tcp_non_target)
    assert(
      "IPv6 non-target TCP does not enter TUN",
      packet_totals(ipv6_tcp_target_stats) == packet_totals(ipv6_tcp_after_non_target),
      { before: packet_totals(ipv6_tcp_target_stats), after: packet_totals(ipv6_tcp_after_non_target) }
    )
    report[:scenarios][:ipv6_tcp_forwarding_and_isolation] = {
      status: "PASS",
      target: ipv6_tcp,
      non_target: ipv6_tcp_non_target,
      target_stats: ipv6_tcp_target_stats,
      after_non_target_stats: ipv6_tcp_after_non_target
    }

    ipv6_udp = target_udp(host: IPV6_HOST, port: IPV6_UDP_PORT)
    ipv6_udp_target_stats = stats
    ipv6_udp_non_target = app_probe(
      "com.interceptproxy.vpn.isolationprobe",
      timeout: 5,
      host: IPV6_HOST,
      port: IPV6_UDP_PORT,
      protocol: "udp"
    )
    ipv6_udp_after_non_target = stats
    assert("IPv6 target UDP forwarding", ipv6_udp[:stdout].include?("UDP-VPN-GATE"), ipv6_udp)
    assert("IPv6 non-target UDP bypass", ipv6_udp_non_target[:stdout].include?("UDP-VPN-GATE"), ipv6_udp_non_target)
    assert(
      "IPv6 non-target UDP does not enter TUN",
      packet_totals(ipv6_udp_target_stats) == packet_totals(ipv6_udp_after_non_target),
      { before: packet_totals(ipv6_udp_target_stats), after: packet_totals(ipv6_udp_after_non_target) }
    )
    report[:scenarios][:ipv6_udp_forwarding_and_isolation] = {
      status: "PASS",
      target: ipv6_udp,
      non_target: ipv6_udp_non_target,
      target_stats: ipv6_udp_target_stats,
      after_non_target_stats: ipv6_udp_after_non_target
    }
  end

  delayed = profile("fixed-delay", fixed_delay_millis: 150)
  apply_profile(delayed)
  tcp = target_http
  delayed_stats = stats
  assert("fixed delay response", tcp[:stdout].include?("VPN-BASELINE"), tcp)
  assert("fixed delay observable", tcp[:elapsed_millis] >= 450, tcp)
  assert("fixed delay counter", delayed_stats["impairment_delay_millis_total"].to_i.positive?, delayed_stats)
  report[:scenarios][:fixed_delay] = { tcp: tcp, stats: delayed_stats }

  jittered = profile("jitter", fixed_delay_millis: 100, uniform_jitter_millis: 60)
  apply_profile(jittered)
  tcp = target_http
  jitter_stats = stats
  assert("jitter response", tcp[:stdout].include?("VPN-BASELINE"), tcp)
  assert("jitter delay counter", jitter_stats["impairment_delay_millis_total"].to_i.positive?, jitter_stats)
  report[:scenarios][:uniform_jitter] = { tcp: tcp, stats: jitter_stats }

  limited = profile("rate-limit", download_bytes_per_second: 8192)
  apply_profile(limited)
  tcp = target_http(path: "/large", timeout: 12)
  rate_stats = stats
  assert("rate limited response", tcp[:stdout].bytesize >= 32_768, { bytes: tcp[:stdout].bytesize, stderr: tcp[:stderr] })
  assert("rate limit duration", tcp[:elapsed_millis] >= 2500, tcp.slice(:elapsed_millis, :exit, :stderr))
  report[:scenarios][:download_rate_limit] = { elapsed_millis: tcp[:elapsed_millis], bytes: tcp[:stdout].bytesize, stats: rate_stats }

  upload_limited = profile("upload-rate-limit", upload_bytes_per_second: 4096)
  apply_profile(upload_limited)
  upload = target_upload
  upload_rate_stats = stats
  assert("upload rate limited response", upload[:stdout].include?("VPN-UPLOAD-8192"), upload)
  assert("upload rate limit duration", upload[:elapsed_millis] >= 1500, upload)
  assert("upload rate limit delay counter", upload_rate_stats["impairment_delay_millis_total"].to_i.positive?, upload_rate_stats)
  report[:scenarios][:upload_rate_limit] = { upload: upload, stats: upload_rate_stats }

  duplicated = profile("duplicate-reorder", duplicate_basis_points: 10_000, reorder_basis_points: 10_000, maximum_reorder_hold_millis: 80)
  apply_profile(duplicated)
  tcp = target_http(timeout: 12)
  duplicate_stats = stats
  assert("duplicate decision", duplicate_stats["impairment_packets_duplicated"].to_i.positive?, duplicate_stats)
  assert("reorder decision", duplicate_stats["impairment_packets_reordered"].to_i.positive?, duplicate_stats)
  report[:scenarios][:duplicate_and_reorder] = { tcp: tcp.slice(:exit, :elapsed_millis, :stderr), stats: duplicate_stats }

  nth = profile("nth-syn", nth_tcp_flag_drops: [{ direction: "upload", flag: "syn", nth: 1 }])
  apply_profile(nth)
  tcp = target_http(timeout: 12)
  nth_stats = stats
  assert("nth SYN retransmission recovered", tcp[:stdout].include?("VPN-BASELINE"), tcp)
  assert("nth SYN was dropped", nth_stats["impairment_packets_dropped"].to_i.positive?, nth_stats)
  assert("nth SYN retransmission observed", nth_stats["upload_tcp_syn_packets"].to_i >= 2, nth_stats)
  report[:scenarios][:nth_tcp_syn_drop_and_retransmission] = { tcp: tcp, stats: nth_stats }

  nth_syn_ack = profile("nth-syn-ack", nth_tcp_flag_drops: [{ direction: "download", flag: "syn_ack", nth: 1 }])
  apply_profile(nth_syn_ack)
  tcp = target_http(timeout: 12)
  syn_ack_stats = stats
  assert("nth SYN-ACK blocks the connection", !tcp[:stdout].include?("VPN-BASELINE"), tcp)
  assert("nth SYN-ACK was dropped", syn_ack_stats["impairment_packets_dropped"].to_i.positive?, syn_ack_stats)
  assert("download SYN-ACK observed", syn_ack_stats["download_tcp_syn_ack_packets"].to_i.positive?, syn_ack_stats)
  report[:scenarios][:nth_tcp_syn_ack_drop] = { tcp: tcp, stats: syn_ack_stats }

  nth_ack = profile("nth-ack", nth_tcp_flag_drops: [{ direction: "upload", flag: "ack", nth: 1 }])
  apply_profile(nth_ack)
  tcp = target_http(timeout: 12)
  ack_stats = stats
  assert("nth ACK drop recovered", tcp[:stdout].include?("VPN-BASELINE"), tcp)
  assert("nth ACK was dropped", ack_stats["impairment_packets_dropped"].to_i.positive?, ack_stats)
  assert("upload ACK packets observed", ack_stats["upload_tcp_ack_packets"].to_i.positive?, ack_stats)
  report[:scenarios][:nth_tcp_ack_drop_recovered] = { tcp: tcp, stats: ack_stats }

  mss = profile("mss-clamp", path_mtu: { mtu: 1280, mss_clamp: 900, mode: "pass" })
  apply_profile(mss)
  tcp = target_http
  mss_stats = stats
  assert("MSS clamp response", tcp[:stdout].include?("VPN-BASELINE"), tcp)
  assert("MSS clamp applied", mss_stats["impairment_mss_clamps"].to_i.positive?, mss_stats)
  report[:scenarios][:mss_clamp] = { tcp: tcp, stats: mss_stats }

  corrupted = profile("corruption", corruption: { probability_basis_points: 10_000, bits_per_packet: 1 })
  apply_profile(corrupted)
  tcp = target_http(timeout: 4)
  corruption_stats = stats
  assert("payload corruption applied", corruption_stats["impairment_packets_corrupted"].to_i.positive?, corruption_stats)
  report[:scenarios][:payload_corruption] = { tcp: tcp.slice(:exit, :elapsed_millis, :stderr), stats: corruption_stats }

  destination_bypass = profile(
    "destination-bypass",
    { random_loss_basis_points: 10_000 },
    destinations: [{ cidr: "192.0.2.1", ports: [] }, { cidr: "198.51.100.0/24", ports: [TCP_PORT] }]
  )
  apply_profile(destination_bypass)
  tcp = target_http
  bypass_stats = stats
  assert("multiple destination bypass", tcp[:stdout].include?("VPN-BASELINE"), tcp)
  assert("unmatched destination not impaired", bypass_stats["impairment_packets_dropped"].to_i.zero?, bypass_stats)
  report[:scenarios][:multiple_destination_bypass] = { tcp: tcp, stats: bypass_stats }

  destination_match = profile(
    "destination-match",
    { random_loss_basis_points: 10_000 },
    destinations: [{ cidr: "192.0.2.1", ports: [] }, { cidr: HOST, ports: [TCP_PORT] }]
  )
  apply_profile(destination_match)
  tcp = target_http(timeout: 3)
  match_stats = stats
  assert("matching destination blocked", !tcp[:stdout].include?("VPN-BASELINE"), tcp)
  assert("matching destination drop counter", match_stats["impairment_packets_dropped"].to_i.positive?, match_stats)
  report[:scenarios][:multiple_destination_match] = { tcp: tcp, stats: match_stats }

  total_loss = profile("total-loss", random_loss_basis_points: 10_000)
  apply_profile(total_loss)
  tcp = target_http(timeout: 3)
  direct = NON_TARGET_PACKAGES.to_h { |package_name| [package_name, non_target_http(package_name)] }
  loss_stats = stats
  assert("target total loss", !tcp[:stdout].include?("VPN-BASELINE"), tcp)
  assert("two non-target apps survive total loss", direct.values.all? { |probe| probe[:stdout].include?("VPN-BASELINE") }, direct)
  assert("ADB survives total loss", run("adb", "-s", SERIAL, "get-state").first.strip == "device")
  assert("total loss counter", loss_stats["impairment_packets_dropped"].to_i.positive?, loss_stats)
  report[:scenarios][:total_loss_target_only] = { tcp: tcp, non_target_uids: non_target_uids, non_targets: direct, stats: loss_stats }

  burst = profile("burst-loss", burst_loss: {
    enter_bad_state_basis_points: 10_000,
    leave_bad_state_basis_points: 0,
    good_state_loss_basis_points: 0,
    bad_state_loss_basis_points: 10_000
  })
  apply_profile(burst)
  tcp = target_http(timeout: 3)
  burst_stats = stats
  assert("burst loss target", !tcp[:stdout].include?("VPN-BASELINE"), tcp)
  assert("burst loss counter", burst_stats["impairment_packets_dropped"].to_i.positive?, burst_stats)
  report[:scenarios][:gilbert_elliott_burst_loss] = { tcp: tcp, stats: burst_stats }

  blackout = profile("blackout", blackout_windows: [{ start_after_millis: 0, duration_millis: 60_000 }])
  apply_profile(blackout)
  tcp = target_http(timeout: 3)
  blackout_stats = stats
  assert("blackout target", !tcp[:stdout].include?("VPN-BASELINE"), tcp)
  assert("blackout counter", blackout_stats["impairment_packets_dropped"].to_i.positive?, blackout_stats)
  report[:scenarios][:blackout_window] = { tcp: tcp, stats: blackout_stats }

  dns = profile("dns-blackhole", dns_blackhole: true)
  apply_profile(dns)
  dns_probe = app_probe(
    TARGET_PACKAGE,
    timeout: 3,
    host: "1.1.1.1",
    protocol: "udp",
    port: 53
  )
  dns_stats = stats
  assert("DNS blackhole counter", dns_stats["impairment_packets_dropped"].to_i.positive?, dns_stats)
  report[:scenarios][:dns_53_blackhole] = { probe: dns_probe, stats: dns_stats }

  dns_853 = profile("dns-853-blackhole", dns_blackhole: true)
  apply_profile(dns_853)
  dns_853_probe = app_probe(
    TARGET_PACKAGE,
    timeout: 3,
    host: "1.1.1.1",
    protocol: "udp",
    port: 853
  )
  dns_853_stats = stats
  assert("DNS 853 blackhole counter", dns_853_stats["impairment_packets_dropped"].to_i.positive?, dns_853_stats)
  report[:scenarios][:dns_853_blackhole] = { probe: dns_853_probe, stats: dns_853_stats }

  pmtu_fragment = profile("pmtu-fragment", path_mtu: { mtu: 576, mss_clamp: nil, mode: "fragment_or_packet_too_big" })
  apply_profile(pmtu_fragment)
  tcp = target_http(path: "/large", timeout: 12)
  fragment_stats = stats
  assert("IPv4 fragmented response", tcp[:stdout].bytesize >= 32_768, { bytes: tcp[:stdout].bytesize, stderr: tcp[:stderr] })
  assert("IPv4 fragments emitted", fragment_stats["impairment_pmtu_fragments"].to_i > 1, fragment_stats)
  report[:scenarios][:ipv4_fragmentation] = { tcp: tcp.slice(:exit, :elapsed_millis, :stderr), bytes: tcp[:stdout].bytesize, stats: fragment_stats }

  pmtu_blackhole = profile("pmtu-blackhole", path_mtu: { mtu: 576, mss_clamp: nil, mode: "blackhole" })
  apply_profile(pmtu_blackhole)
  tcp = target_http(path: "/large", timeout: 5)
  pmtu_stats = stats
  assert("PMTU blackhole drop", pmtu_stats["impairment_packets_dropped"].to_i.positive?, pmtu_stats)
  report[:scenarios][:pmtu_blackhole] = { tcp: tcp.slice(:exit, :elapsed_millis, :stderr), stats: pmtu_stats }

  pmtu_signal = profile("pmtu-signal", path_mtu: { mtu: 576, mss_clamp: nil, mode: "signal_too_big" })
  apply_profile(pmtu_signal)
  target_http(path: "/large", timeout: 5)
  signal_stats = stats
  report[:scenarios][:pmtu_signal] = { stats: signal_stats }
  assert(
    "PMTU signal/fragment data plane implementation",
    signal_stats["impairment_unimplemented_pmtu_actions"].to_i.zero?,
    signal_stats
  )

  stop_recovery = stop_and_verify_recovery
  report[:scenarios][:stop_vpn_recovers_network_within_five_seconds] = stop_recovery
  report[:stop_recovery_millis] = stop_recovery[:elapsed_millis]
  assert("stop VPN recovers network within five seconds", stop_recovery[:status] == "PASS", stop_recovery)

  report[:result] = report[:skips].empty? ? "PASS" : "PASS_WITH_SKIPS"
rescue StandardError => error
  report[:result] = "FAIL"
  report[:error] = "#{error.class}: #{error.message}"
ensure
  # 主验证中的 stop 会检查返回值和恢复时限；这里仅作幂等清理，避免前序场景失败后遗留 TUN。
  control("stop") rescue nil
  report[:finished_at] = Time.now.iso8601
  report_dir = File.expand_path("reports", __dir__)
  Dir.mkdir(report_dir) unless Dir.exist?(report_dir)
  File.write(File.join(report_dir, "latest.json"), JSON.pretty_generate(report))
  tcp_server.close rescue nil
  udp_server.close rescue nil
  ipv6_tcp_server&.close rescue nil
  ipv6_udp_server&.close rescue nil
  servers.each(&:kill)
end

puts JSON.pretty_generate(report)
exit(%w[PASS PASS_WITH_SKIPS].include?(report[:result]) ? 0 : 1)
