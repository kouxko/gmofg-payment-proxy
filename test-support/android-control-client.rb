# frozen_string_literal: true

# Android Companion 的测试控制客户端。
#
# VpnService 停止后，Android 可能回收只剩 Application 的 Companion 进程。桌面端正式
# 适配器会先唤醒 AdbControlActivity，并对控制 socket 的瞬态关闭执行有限重试；门禁必须
# 使用相同语义，避免把进程回收误判成协议实现错误，也避免无限重试掩盖真实故障。

require "json"
require "open3"
require "socket"
require "timeout"

module AndroidControlClient
  SOCKET_NAME = "intercept_proxy_vpn"
  MAX_ATTEMPTS = 3
  module_function

  def request(serial:, operation:, profile: nil)
    last_error = nil
    MAX_ATTEMPTS.times do |attempt|
      # PackageManager 与 Activity 启动在 APK 覆盖安装或进程重建窗口内可能短暂返回
      # Error type 3。唤醒只是帮助恢复进程，不是协议成功条件；真正的 socket 交换仍会
      # 在下面 fail-closed，并且最多尝试三次。
      wake(serial) if attempt.positive?
      begin
        return request_once(serial: serial, operation: operation, profile: profile)
      rescue EOFError, IOError, SystemCallError, Timeout::Error, RuntimeError => error
        raise unless transient?(error) && attempt + 1 < MAX_ATTEMPTS

        last_error = error
        sleep(0.15 * (attempt + 1))
      end
    end
    raise last_error || "设备端控制通道重试耗尽"
  end

  def request_once(serial:, operation:, profile:)
    port = adb(serial, "forward", "tcp:0", "localabstract:#{SOCKET_NAME}").strip
    request = {
      version: 1,
      request_id: "gate-#{operation}-#{Process.clock_gettime(Process::CLOCK_MONOTONIC, :millisecond)}",
      operation: operation,
      payload: profile.nil? ? {} : { profile: profile }
    }.to_json
    socket = TCPSocket.new("127.0.0.1", Integer(port))
    response = Timeout.timeout(8) do
      socket.write([request.bytesize].pack("N"))
      socket.write(request)
      prefix = socket.read(4)
      raise EOFError, "control response did not contain a length prefix" if prefix.nil? || prefix.bytesize != 4

      length = prefix.unpack1("N")
      payload = socket.read(length)
      raise EOFError, "control response body was truncated" if payload.nil? || payload.bytesize != length

      JSON.parse(payload)
    end
    raise "control #{operation} failed: #{response}" unless response.fetch("ok")

    response.fetch("status")
  ensure
    socket&.close
    adb(serial, "forward", "--remove", "tcp:#{port}", allow_failure: true) unless port.nil?
  end

  def wake(serial)
    adb(
      serial,
      "shell",
      "am",
      "start",
      "-W",
      "-n",
      "com.interceptproxy.vpn/.AdbControlActivity",
      "--es",
      "command",
      "wake_control_server"
    )
  rescue RuntimeError
    nil
  end

  def adb(serial, *arguments, allow_failure: false)
    stdout, stderr, status = Open3.capture3("adb", "-s", serial, *arguments)
    return stdout if status.success? || allow_failure

    raise "adb command failed (#{status.exitstatus}): #{stderr.empty? ? stdout : stderr}"
  end

  def transient?(error)
    error.is_a?(EOFError) || error.is_a?(IOError) || error.is_a?(SystemCallError) ||
      error.is_a?(Timeout::Error) || error.message.include?("length prefix")
  end
end
