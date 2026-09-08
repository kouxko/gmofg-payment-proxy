CUA observed a blank initial window followed by a complete Workspace UI. Rendering time was not sampled continuously and no product root cause was established.

The second App start retained the selected Workspace, both listeners and all five rule definitions. Runtime entries were empty immediately after restart. Both listeners were then started again through CUA and showed running on 2026-09-08 10:38:11 +08:00.

CUA intermittently returned `cgWindowNotFound`, `SCStreamErrorDomain -3811`, or an accessibility tree without HTML nodes while screenshots showed the page. One direct `entry_status_list` query timed out; subsequent UI and listening-port observations confirmed running entries. These diagnostic attempts are not wire-test PASS evidence.
