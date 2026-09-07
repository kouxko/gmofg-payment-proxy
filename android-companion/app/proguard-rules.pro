# JNI 会按完整类名和方法名查找这些入口，混淆时必须保留。
-keep class com.interceptproxy.vpn.NativeBridge { *; }
-keep class com.interceptproxy.vpn.NativeSocketProtector { *; }
