# Exchange error and observation regressions

Baseline core tests failed twice: required Server EOF returned untyped Error, and Socket external failure omitted correlation fields. A real loopback TCP test failed with terminal PROCESSING_FAILED/FrameProcess/no direction after sending 03616263, Server receiving 55616263 and returning EOF.

Fix: retain existing READ_FAILED classification (pipeline.rs already maps Read to this code), Read and Downstream in the core Error; reuse observation::failed and remove its duplicate tracing helper. Add Socket external_* fields identical to existing HTTP projection.

RED: exchange-red.log 25 pass / 2 fail; socket-eof-red.log 1 fail, all assertions reached actual production behavior. GREEN: exchange-green.log 27 pass; socket-green.log 63 pass. Associated JSON files store exact commands and start/end times. Real TCP test uses local synthetic ScriptedFactory from socket_relay/tests/support.rs: Frame length byte; Decode/Rules/Encode preserve payload and replace prefix with U or D. No external package or Android/remote business was contacted.

resources contains actual owned production and regression test snapshots. Full dependency/config snapshots will be in final task evidence. Final observation comment was condensed after focused GREEN for <=500 line contract; no executable behavior changed.
