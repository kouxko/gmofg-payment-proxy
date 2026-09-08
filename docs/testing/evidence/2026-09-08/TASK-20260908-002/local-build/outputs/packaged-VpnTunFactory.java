package com.interceptproxy.vpn;

import android.net.VpnService;
import android.os.ParcelFileDescriptor;
import android.util.Log;
import java.util.Iterator;
import kotlin.Metadata;
import kotlin.Result;
import kotlin.ResultKt;
import kotlin.collections.CollectionsKt;
import kotlin.jvm.functions.Function1;
import kotlin.jvm.internal.Intrinsics;
import kotlin.uuid.Uuid;

/* JADX INFO: compiled from: VpnTunFactory.kt */
/* JADX INFO: loaded from: classes.dex */
@Metadata(d1 = {"\u0000(\n\u0002\u0018\u0002\n\u0002\u0010\u0000\n\u0002\b\u0003\n\u0002\u0010\u000e\n\u0000\n\u0002\u0018\u0002\n\u0000\n\u0002\u0018\u0002\n\u0002\u0018\u0002\n\u0000\n\u0002\u0018\u0002\n\u0000\bÀ\u0002\u0018\u00002\u00020\u0001B\t\b\u0002¢\u0006\u0004\b\u0002\u0010\u0003J\u001c\u0010\u0006\u001a\u0004\u0018\u00010\u00072\n\u0010\b\u001a\u00060\tR\u00020\n2\u0006\u0010\u000b\u001a\u00020\fR\u000e\u0010\u0004\u001a\u00020\u0005X\u0082T¢\u0006\u0002\n\u0000¨\u0006\r"}, d2 = {"Lcom/interceptproxy/vpn/VpnTunFactory;", "", "<init>", "()V", "TAG", "", "establish", "Landroid/os/ParcelFileDescriptor;", "builder", "Landroid/net/VpnService$Builder;", "Landroid/net/VpnService;", "profile", "Lcom/interceptproxy/vpn/CompanionProfile;", "app"}, k = 1, mv = {2, 2, 0}, xi = 48)
public final class VpnTunFactory {
    public static final VpnTunFactory INSTANCE = new VpnTunFactory();
    private static final String TAG = "InterceptVpnService";

    private VpnTunFactory() {
    }

    public final ParcelFileDescriptor establish(VpnService.Builder builder, CompanionProfile profile) {
        Object objM13constructorimpl;
        Intrinsics.checkNotNullParameter(builder, "builder");
        Intrinsics.checkNotNullParameter(profile, "profile");
        try {
            Result.Companion companion = Result.INSTANCE;
            VpnTunFactory vpnTunFactory = this;
            VpnService.Builder builderAddRoute = builder.setSession(CollectionsKt.joinToString$default(profile.getTargetPackages(), null, null, null, 0, null, new Function1() { // from class: com.interceptproxy.vpn.VpnTunFactory$$ExternalSyntheticLambda0
                @Override // kotlin.jvm.functions.Function1
                public final Object invoke(Object obj) {
                    return VpnTunFactory.establish$lambda$1$lambda$0((TargetPackage) obj);
                }
            }, 31, null)).allowBypass().setMtu(profile.getMtu()).addAddress("10.254.0.2", 32).addAddress("fd00:6970:7670::2", Uuid.SIZE_BITS).addRoute("0.0.0.0", 0).addRoute("::", 0);
            Intrinsics.checkNotNullExpressionValue(builderAddRoute, "addRoute(...)");
            VpnLocalDns.INSTANCE.addTo(builderAddRoute);
            Iterator<TargetPackage> it = profile.getTargetPackages().iterator();
            while (it.hasNext()) {
                builder.addAllowedApplication(it.next().getPackageName());
            }
            objM13constructorimpl = Result.m13constructorimpl(builder.establish());
        } catch (Throwable th) {
            Result.Companion companion2 = Result.INSTANCE;
            objM13constructorimpl = Result.m13constructorimpl(ResultKt.createFailure(th));
        }
        Throwable thM16exceptionOrNullimpl = Result.m16exceptionOrNullimpl(objM13constructorimpl);
        if (thM16exceptionOrNullimpl != null) {
            Log.e(TAG, "建立 Android TUN 失败", thM16exceptionOrNullimpl);
            objM13constructorimpl = null;
        }
        return (ParcelFileDescriptor) objM13constructorimpl;
    }

    static final CharSequence establish$lambda$1$lambda$0(TargetPackage it) {
        Intrinsics.checkNotNullParameter(it, "it");
        return it.getPackageName();
    }
}
