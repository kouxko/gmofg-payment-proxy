package com.android.server.connectivity;

import android.R;
import android.app.AppGlobals;
import android.app.AppOpsManager;
import android.app.Notification;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.ServiceConnection;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.content.pm.UserInfo;
import android.net.ConnectivityManager;
import android.net.INetworkManagementEventObserver;
import android.net.InetAddresses;
import android.net.IpPrefix;
import android.net.LinkAddress;
import android.net.LinkProperties;
import android.net.LocalSocket;
import android.net.LocalSocketAddress;
import android.net.Network;
import android.net.NetworkAgent;
import android.net.NetworkCapabilities;
import android.net.NetworkInfo;
import android.net.NetworkMisc;
import android.net.NetworkUtils;
import android.net.RouteInfo;
import android.net.UidRange;
import android.os.Binder;
import android.os.Bundle;
import android.os.FileUtils;
import android.os.IBinder;
import android.os.INetworkManagementService;
import android.os.Looper;
import android.os.Parcel;
import android.os.ParcelFileDescriptor;
import android.os.Process;
import android.os.RemoteException;
import android.os.SystemClock;
import android.os.SystemService;
import android.os.UserHandle;
import android.os.UserManager;
import android.provider.Settings;
import android.security.KeyStore;
import android.text.TextUtils;
import android.util.ArraySet;
import android.util.Log;
import com.android.internal.annotations.GuardedBy;
import com.android.internal.annotations.VisibleForTesting;
import com.android.internal.net.LegacyVpnInfo;
import com.android.internal.net.VpnConfig;
import com.android.internal.net.VpnInfo;
import com.android.internal.net.VpnProfile;
import com.android.internal.notification.SystemNotificationChannels;
import com.android.internal.util.ArrayUtils;
import com.android.server.DeviceIdleController;
import com.android.server.LocalServices;
import com.android.server.net.BaseNetworkObserver;
import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.math.BigInteger;
import java.net.Inet4Address;
import java.net.Inet6Address;
import java.net.InetAddress;
import java.net.UnknownHostException;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collection;
import java.util.Collections;
import java.util.Comparator;
import java.util.Iterator;
import java.util.List;
import java.util.Objects;
import java.util.Set;
import java.util.SortedSet;
import java.util.TreeSet;
import java.util.concurrent.atomic.AtomicInteger;
import libcore.io.IoUtils;

/* JADX INFO: loaded from: classes.dex */
public class Vpn {
    private static final boolean LOGD = true;
    private static final int MAX_ROUTES_TO_EVALUATE = 150;
    private static final long MOST_IPV4_ADDRESSES_COUNT = 3650722201L;
    private static final BigInteger MOST_IPV6_ADDRESSES_COUNT;
    private static final String NETWORKTYPE = "VPN";
    private static final String TAG = "Vpn";
    private static final long VPN_LAUNCH_IDLE_WHITELIST_DURATION_MS = 60000;
    private boolean mAlwaysOn;

    @GuardedBy({"this"})
    private Set<UidRange> mBlockedUsers;

    @VisibleForTesting
    protected VpnConfig mConfig;
    private Connection mConnection;
    private final Context mContext;
    private volatile boolean mEnableTeardown;
    private String mInterface;
    private boolean mIsPackageTargetingAtLeastQ;
    private LegacyVpnRunner mLegacyVpnRunner;
    private boolean mLockdown;
    private List<String> mLockdownWhitelist;
    private final Looper mLooper;
    private final INetworkManagementService mNetd;

    @VisibleForTesting
    protected NetworkAgent mNetworkAgent;

    @VisibleForTesting
    protected final NetworkCapabilities mNetworkCapabilities;
    private final NetworkInfo mNetworkInfo;
    private INetworkManagementEventObserver mObserver;
    private int mOwnerUID;
    private String mPackage;
    private PendingIntent mStatusIntent;
    private final SystemServices mSystemServices;
    private final int mUserHandle;

    private native boolean jniAddAddress(String str, String str2, int i);

    /* JADX INFO: Access modifiers changed from: private */
    public native int jniCheck(String str);

    private native int jniCreate(int i);

    private native boolean jniDelAddress(String str, String str2, int i);

    private native String jniGetName(int i);

    private native void jniReset(String str);

    private native int jniSetAddresses(String str, String str2);

    static {
        BigInteger twoPower128 = BigInteger.ONE.shiftLeft(128);
        MOST_IPV6_ADDRESSES_COUNT = twoPower128.multiply(BigInteger.valueOf(85L)).divide(BigInteger.valueOf(100L));
    }

    public Vpn(Looper looper, Context context, INetworkManagementService netService, int userHandle) {
        this(looper, context, netService, userHandle, new SystemServices(context));
    }

    @VisibleForTesting
    protected Vpn(Looper looper, Context context, INetworkManagementService netService, int userHandle, SystemServices systemServices) {
        this.mEnableTeardown = true;
        this.mAlwaysOn = false;
        this.mLockdown = false;
        this.mLockdownWhitelist = Collections.emptyList();
        this.mBlockedUsers = new ArraySet();
        this.mObserver = new BaseNetworkObserver() { // from class: com.android.server.connectivity.Vpn.2
            public void interfaceStatusChanged(String interfaze, boolean up) {
                synchronized (Vpn.this) {
                    if (!up) {
                        if (Vpn.this.mLegacyVpnRunner != null) {
                            Vpn.this.mLegacyVpnRunner.check(interfaze);
                        }
                    }
                }
            }

            public void interfaceRemoved(String interfaze) {
                synchronized (Vpn.this) {
                    if (interfaze.equals(Vpn.this.mInterface) && Vpn.this.jniCheck(interfaze) == 0) {
                        Vpn.this.mStatusIntent = null;
                        Vpn.this.mNetworkCapabilities.setUids(null);
                        Vpn.this.mConfig = null;
                        Vpn.this.mInterface = null;
                        if (Vpn.this.mConnection != null) {
                            Vpn.this.mContext.unbindService(Vpn.this.mConnection);
                            Vpn.this.mConnection = null;
                            Vpn.this.agentDisconnect();
                        } else if (Vpn.this.mLegacyVpnRunner != null) {
                            Vpn.this.mLegacyVpnRunner.exit();
                            Vpn.this.mLegacyVpnRunner = null;
                        }
                    }
                }
            }
        };
        this.mContext = context;
        this.mNetd = netService;
        this.mUserHandle = userHandle;
        this.mLooper = looper;
        this.mSystemServices = systemServices;
        this.mPackage = "[Legacy VPN]";
        this.mOwnerUID = getAppUid(this.mPackage, this.mUserHandle);
        this.mIsPackageTargetingAtLeastQ = doesPackageTargetAtLeastQ(this.mPackage);
        try {
            netService.registerObserver(this.mObserver);
        } catch (RemoteException e) {
            Log.wtf(TAG, "Problem registering observer", e);
        }
        this.mNetworkInfo = new NetworkInfo(17, 0, NETWORKTYPE, "");
        this.mNetworkCapabilities = new NetworkCapabilities();
        this.mNetworkCapabilities.addTransportType(4);
        this.mNetworkCapabilities.removeCapability(15);
        updateCapabilities(null);
        loadAlwaysOnPackage();
    }

    public void setEnableTeardown(boolean enableTeardown) {
        this.mEnableTeardown = enableTeardown;
    }

    @VisibleForTesting
    protected void updateState(NetworkInfo.DetailedState detailedState, String reason) {
        Log.d(TAG, "setting state=" + detailedState + ", reason=" + reason);
        this.mNetworkInfo.setDetailedState(detailedState, reason, null);
        NetworkAgent networkAgent = this.mNetworkAgent;
        if (networkAgent != null) {
            networkAgent.sendNetworkInfo(this.mNetworkInfo);
        }
        updateAlwaysOnNotification(detailedState);
    }

    public synchronized NetworkCapabilities updateCapabilities(Network defaultNetwork) {
        if (this.mConfig == null) {
            return null;
        }
        Network[] underlyingNetworks = this.mConfig.underlyingNetworks;
        boolean isAlwaysMetered = false;
        if (underlyingNetworks == null && defaultNetwork != null) {
            underlyingNetworks = new Network[]{defaultNetwork};
        }
        if (this.mIsPackageTargetingAtLeastQ && this.mConfig.isMetered) {
            isAlwaysMetered = true;
        }
        applyUnderlyingCapabilities((ConnectivityManager) this.mContext.getSystemService(ConnectivityManager.class), underlyingNetworks, this.mNetworkCapabilities, isAlwaysMetered);
        return new NetworkCapabilities(this.mNetworkCapabilities);
    }

    @VisibleForTesting
    public static void applyUnderlyingCapabilities(ConnectivityManager cm, Network[] underlyingNetworks, NetworkCapabilities caps, boolean isAlwaysMetered) {
        boolean hadUnderlyingNetworks;
        Network[] networkArr = underlyingNetworks;
        boolean z = true;
        int[] transportTypes = {4};
        int downKbps = 0;
        int downKbps2 = 0;
        boolean metered = isAlwaysMetered;
        boolean metered2 = false;
        boolean roaming = false;
        if (networkArr == null) {
            hadUnderlyingNetworks = false;
        } else {
            int length = networkArr.length;
            hadUnderlyingNetworks = false;
            boolean congested = false;
            boolean roaming2 = false;
            boolean metered3 = metered;
            int upKbps = 0;
            int downKbps3 = 0;
            int[] transportTypes2 = transportTypes;
            int i = 0;
            while (i < length) {
                Network underlying = networkArr[i];
                NetworkCapabilities underlyingCaps = cm.getNetworkCapabilities(underlying);
                if (underlyingCaps != null) {
                    hadUnderlyingNetworks = true;
                    int[] transportTypes3 = underlyingCaps.getTransportTypes();
                    int length2 = transportTypes3.length;
                    int[] transportTypes4 = transportTypes2;
                    int i2 = 0;
                    while (i2 < length2) {
                        Network underlying2 = underlying;
                        int underlyingType = transportTypes3[i2];
                        transportTypes4 = ArrayUtils.appendInt(transportTypes4, underlyingType);
                        i2++;
                        underlying = underlying2;
                    }
                    downKbps3 = NetworkCapabilities.minBandwidth(downKbps3, underlyingCaps.getLinkDownstreamBandwidthKbps());
                    upKbps = NetworkCapabilities.minBandwidth(upKbps, underlyingCaps.getLinkUpstreamBandwidthKbps());
                    z = true;
                    metered3 |= !underlyingCaps.hasCapability(11);
                    roaming2 |= !underlyingCaps.hasCapability(18);
                    congested |= !underlyingCaps.hasCapability(20);
                    transportTypes2 = transportTypes4;
                }
                i++;
                networkArr = underlyingNetworks;
            }
            transportTypes = transportTypes2;
            downKbps = downKbps3;
            downKbps2 = upKbps;
            metered = metered3;
            metered2 = roaming2;
            roaming = congested;
        }
        if (!hadUnderlyingNetworks) {
            metered = true;
            metered2 = false;
            roaming = false;
        }
        caps.setTransportTypes(transportTypes);
        caps.setLinkDownstreamBandwidthKbps(downKbps);
        caps.setLinkUpstreamBandwidthKbps(downKbps2);
        caps.setCapability(11, !metered ? z : false);
        caps.setCapability(18, !metered2 ? z : false);
        if (roaming) {
            z = false;
        }
        caps.setCapability(20, z);
    }

    public synchronized void setLockdown(boolean lockdown) {
        enforceControlPermissionOrInternalCaller();
        setVpnForcedLocked(lockdown);
        this.mLockdown = lockdown;
        if (this.mAlwaysOn) {
            saveAlwaysOnPackage();
        }
    }

    public synchronized boolean getLockdown() {
        return this.mLockdown;
    }

    public synchronized boolean getAlwaysOn() {
        return this.mAlwaysOn;
    }

    public boolean isAlwaysOnPackageSupported(String packageName) {
        enforceSettingsPermission();
        if (packageName == null) {
            return false;
        }
        PackageManager pm = this.mContext.getPackageManager();
        ApplicationInfo appInfo = null;
        try {
            appInfo = pm.getApplicationInfoAsUser(packageName, 0, this.mUserHandle);
        } catch (PackageManager.NameNotFoundException e) {
            Log.w(TAG, "Can't find \"" + packageName + "\" when checking always-on support");
        }
        if (appInfo == null || appInfo.targetSdkVersion < 24) {
            return false;
        }
        Intent intent = new Intent("android.net.VpnService");
        intent.setPackage(packageName);
        List<ResolveInfo> services = pm.queryIntentServicesAsUser(intent, 128, this.mUserHandle);
        if (services == null || services.size() == 0) {
            return false;
        }
        for (ResolveInfo rInfo : services) {
            Bundle metaData = rInfo.serviceInfo.metaData;
            if (metaData != null && !metaData.getBoolean("android.net.VpnService.SUPPORTS_ALWAYS_ON", true)) {
                return false;
            }
        }
        return true;
    }

    public synchronized boolean setAlwaysOnPackage(String packageName, boolean lockdown, List<String> lockdownWhitelist) {
        enforceControlPermissionOrInternalCaller();
        if (setAlwaysOnPackageInternal(packageName, lockdown, lockdownWhitelist)) {
            saveAlwaysOnPackage();
            return true;
        }
        return false;
    }

    @GuardedBy({"this"})
    private boolean setAlwaysOnPackageInternal(String packageName, boolean lockdown, List<String> lockdownWhitelist) {
        List<String> listEmptyList;
        boolean z = false;
        if ("[Legacy VPN]".equals(packageName)) {
            Log.w(TAG, "Not setting legacy VPN \"" + packageName + "\" as always-on.");
            return false;
        }
        if (lockdownWhitelist != null) {
            for (String pkg : lockdownWhitelist) {
                if (pkg.contains(",")) {
                    Log.w(TAG, "Not setting always-on vpn, invalid whitelisted package: " + pkg);
                    return false;
                }
            }
        }
        if (packageName != null) {
            if (!setPackageAuthorization(packageName, true)) {
                return false;
            }
            this.mAlwaysOn = true;
        } else {
            packageName = "[Legacy VPN]";
            this.mAlwaysOn = false;
        }
        if (this.mAlwaysOn && lockdown) {
            z = true;
        }
        this.mLockdown = z;
        if (this.mLockdown && lockdownWhitelist != null) {
            listEmptyList = Collections.unmodifiableList(new ArrayList(lockdownWhitelist));
        } else {
            listEmptyList = Collections.emptyList();
        }
        this.mLockdownWhitelist = listEmptyList;
        if (isCurrentPreparedPackage(packageName)) {
            updateAlwaysOnNotification(this.mNetworkInfo.getDetailedState());
            setVpnForcedLocked(this.mLockdown);
        } else {
            prepareInternal(packageName);
        }
        return true;
    }

    private static boolean isNullOrLegacyVpn(String packageName) {
        return packageName == null || "[Legacy VPN]".equals(packageName);
    }

    public synchronized String getAlwaysOnPackage() {
        enforceControlPermissionOrInternalCaller();
        return this.mAlwaysOn ? this.mPackage : null;
    }

    public synchronized List<String> getLockdownWhitelist() {
        return this.mLockdown ? this.mLockdownWhitelist : null;
    }

    @GuardedBy({"this"})
    private void saveAlwaysOnPackage() {
        long token = Binder.clearCallingIdentity();
        try {
            this.mSystemServices.settingsSecurePutStringForUser("always_on_vpn_app", getAlwaysOnPackage(), this.mUserHandle);
            this.mSystemServices.settingsSecurePutIntForUser("always_on_vpn_lockdown", (this.mAlwaysOn && this.mLockdown) ? 1 : 0, this.mUserHandle);
            this.mSystemServices.settingsSecurePutStringForUser("always_on_vpn_lockdown_whitelist", String.join(",", this.mLockdownWhitelist), this.mUserHandle);
        } finally {
            Binder.restoreCallingIdentity(token);
        }
    }

    @GuardedBy({"this"})
    private void loadAlwaysOnPackage() {
        long token = Binder.clearCallingIdentity();
        try {
            String alwaysOnPackage = this.mSystemServices.settingsSecureGetStringForUser("always_on_vpn_app", this.mUserHandle);
            boolean alwaysOnLockdown = this.mSystemServices.settingsSecureGetIntForUser("always_on_vpn_lockdown", 0, this.mUserHandle) != 0;
            String whitelistString = this.mSystemServices.settingsSecureGetStringForUser("always_on_vpn_lockdown_whitelist", this.mUserHandle);
            List<String> whitelistedPackages = TextUtils.isEmpty(whitelistString) ? Collections.emptyList() : Arrays.asList(whitelistString.split(","));
            setAlwaysOnPackageInternal(alwaysOnPackage, alwaysOnLockdown, whitelistedPackages);
        } finally {
            Binder.restoreCallingIdentity(token);
        }
    }

    public boolean startAlwaysOnVpn() {
        synchronized (this) {
            String alwaysOnPackage = getAlwaysOnPackage();
            if (alwaysOnPackage == null) {
                return true;
            }
            if (!isAlwaysOnPackageSupported(alwaysOnPackage)) {
                setAlwaysOnPackage(null, false, null);
                return false;
            }
            if (getNetworkInfo().isConnected()) {
                return true;
            }
            long oldId = Binder.clearCallingIdentity();
            try {
                DeviceIdleController.LocalService idleController = (DeviceIdleController.LocalService) LocalServices.getService(DeviceIdleController.LocalService.class);
                idleController.addPowerSaveTempWhitelistApp(Process.myUid(), alwaysOnPackage, 60000L, this.mUserHandle, false, "vpn");
                Intent serviceIntent = new Intent("android.net.VpnService");
                serviceIntent.setPackage(alwaysOnPackage);
                try {
                    boolean z = this.mContext.startServiceAsUser(serviceIntent, UserHandle.of(this.mUserHandle)) != null;
                    Binder.restoreCallingIdentity(oldId);
                    return z;
                } catch (RuntimeException e) {
                    Log.e(TAG, "VpnService " + serviceIntent + " failed to start", e);
                    Binder.restoreCallingIdentity(oldId);
                    return false;
                }
            } catch (Throwable th) {
                Binder.restoreCallingIdentity(oldId);
                throw th;
            }
        }
    }

    public synchronized boolean prepare(String oldPackage, String newPackage) {
        if (oldPackage != null) {
            if (this.mAlwaysOn && !isCurrentPreparedPackage(oldPackage)) {
                return false;
            }
            if (!isCurrentPreparedPackage(oldPackage)) {
                if (oldPackage.equals("[Legacy VPN]") || !isVpnUserPreConsented(oldPackage)) {
                    return false;
                }
                prepareInternal(oldPackage);
                return true;
            }
            if (!oldPackage.equals("[Legacy VPN]") && !isVpnUserPreConsented(oldPackage)) {
                prepareInternal("[Legacy VPN]");
                return false;
            }
        }
        if (newPackage != null && (newPackage.equals("[Legacy VPN]") || !isCurrentPreparedPackage(newPackage))) {
            enforceControlPermission();
            if (this.mAlwaysOn && !isCurrentPreparedPackage(newPackage)) {
                return false;
            }
            prepareInternal(newPackage);
            return true;
        }
        return true;
    }

    private boolean isCurrentPreparedPackage(String packageName) {
        return getAppUid(packageName, this.mUserHandle) == this.mOwnerUID;
    }

    private void prepareInternal(String newPackage) {
        long token = Binder.clearCallingIdentity();
        try {
            if (this.mInterface != null) {
                this.mStatusIntent = null;
                agentDisconnect();
                jniReset(this.mInterface);
                this.mInterface = null;
                this.mNetworkCapabilities.setUids(null);
            }
            if (this.mConnection != null) {
                try {
                    this.mConnection.mService.transact(16777215, Parcel.obtain(), null, 1);
                } catch (Exception e) {
                }
                this.mContext.unbindService(this.mConnection);
                this.mConnection = null;
            } else if (this.mLegacyVpnRunner != null) {
                this.mLegacyVpnRunner.exit();
                this.mLegacyVpnRunner = null;
            }
            try {
                this.mNetd.denyProtect(this.mOwnerUID);
            } catch (Exception e2) {
                Log.wtf(TAG, "Failed to disallow UID " + this.mOwnerUID + " to call protect() " + e2);
            }
            Log.i(TAG, "Switched from " + this.mPackage + " to " + newPackage);
            this.mPackage = newPackage;
            this.mOwnerUID = getAppUid(newPackage, this.mUserHandle);
            this.mIsPackageTargetingAtLeastQ = doesPackageTargetAtLeastQ(newPackage);
            try {
                this.mNetd.allowProtect(this.mOwnerUID);
            } catch (Exception e3) {
                Log.wtf(TAG, "Failed to allow UID " + this.mOwnerUID + " to call protect() " + e3);
            }
            this.mConfig = null;
            updateState(NetworkInfo.DetailedState.IDLE, "prepare");
            setVpnForcedLocked(this.mLockdown);
            Binder.restoreCallingIdentity(token);
        } catch (Throwable th) {
            Binder.restoreCallingIdentity(token);
            throw th;
        }
    }

    public boolean setPackageAuthorization(String packageName, boolean authorized) {
        enforceControlPermissionOrInternalCaller();
        int uid = getAppUid(packageName, this.mUserHandle);
        if (uid == -1 || "[Legacy VPN]".equals(packageName)) {
            return false;
        }
        long token = Binder.clearCallingIdentity();
        try {
            AppOpsManager appOps = (AppOpsManager) this.mContext.getSystemService("appops");
            appOps.setMode(47, uid, packageName, authorized ? 0 : 1);
            return true;
        } catch (Exception e) {
            Log.wtf(TAG, "Failed to set app ops for package " + packageName + ", uid " + uid, e);
            return false;
        } finally {
            Binder.restoreCallingIdentity(token);
        }
    }

    private boolean isVpnUserPreConsented(String packageName) {
        AppOpsManager appOps = (AppOpsManager) this.mContext.getSystemService("appops");
        return appOps.noteOpNoThrow(47, Binder.getCallingUid(), packageName) == 0;
    }

    private int getAppUid(String app, int userHandle) {
        if ("[Legacy VPN]".equals(app)) {
            return Process.myUid();
        }
        PackageManager pm = this.mContext.getPackageManager();
        try {
            int result = pm.getPackageUidAsUser(app, userHandle);
            return result;
        } catch (PackageManager.NameNotFoundException e) {
            return -1;
        }
    }

    private boolean doesPackageTargetAtLeastQ(String packageName) {
        if ("[Legacy VPN]".equals(packageName)) {
            return true;
        }
        PackageManager pm = this.mContext.getPackageManager();
        try {
            ApplicationInfo appInfo = pm.getApplicationInfoAsUser(packageName, 0, this.mUserHandle);
            return appInfo.targetSdkVersion >= 29;
        } catch (PackageManager.NameNotFoundException e) {
            Log.w(TAG, "Can't find \"" + packageName + "\"");
            return false;
        }
    }

    public NetworkInfo getNetworkInfo() {
        return this.mNetworkInfo;
    }

    public int getNetId() {
        NetworkAgent networkAgent = this.mNetworkAgent;
        if (networkAgent != null) {
            return networkAgent.netId;
        }
        return 0;
    }

    private LinkProperties makeLinkProperties() {
        boolean allowIPv4 = this.mConfig.allowIPv4;
        boolean allowIPv6 = this.mConfig.allowIPv6;
        LinkProperties lp = new LinkProperties();
        lp.setInterfaceName(this.mInterface);
        if (this.mConfig.addresses != null) {
            for (LinkAddress address : this.mConfig.addresses) {
                lp.addLinkAddress(address);
                allowIPv4 |= address.getAddress() instanceof Inet4Address;
                allowIPv6 |= address.getAddress() instanceof Inet6Address;
            }
        }
        if (this.mConfig.routes != null) {
            for (RouteInfo route : this.mConfig.routes) {
                lp.addRoute(route);
                InetAddress address2 = route.getDestination().getAddress();
                allowIPv4 |= address2 instanceof Inet4Address;
                allowIPv6 |= address2 instanceof Inet6Address;
            }
        }
        if (this.mConfig.dnsServers != null) {
            for (String dnsServer : this.mConfig.dnsServers) {
                InetAddress address3 = InetAddress.parseNumericAddress(dnsServer);
                lp.addDnsServer(address3);
                allowIPv4 |= address3 instanceof Inet4Address;
                allowIPv6 |= address3 instanceof Inet6Address;
            }
        }
        lp.setHttpProxy(this.mConfig.proxyInfo);
        if (!allowIPv4) {
            lp.addRoute(new RouteInfo(new IpPrefix(Inet4Address.ANY, 0), 7));
        }
        if (!allowIPv6) {
            lp.addRoute(new RouteInfo(new IpPrefix(Inet6Address.ANY, 0), 7));
        }
        StringBuilder buffer = new StringBuilder();
        if (this.mConfig.searchDomains != null) {
            for (String domain : this.mConfig.searchDomains) {
                buffer.append(domain);
                buffer.append(' ');
            }
        }
        lp.setDomains(buffer.toString().trim());
        return lp;
    }

    @VisibleForTesting
    static boolean providesRoutesToMostDestinations(LinkProperties lp) {
        List<RouteInfo> routes = lp.getAllRoutes();
        if (routes.size() > 150) {
            return true;
        }
        Comparator<IpPrefix> prefixLengthComparator = IpPrefix.lengthComparator();
        TreeSet<IpPrefix> ipv4Prefixes = new TreeSet<>(prefixLengthComparator);
        TreeSet<IpPrefix> ipv6Prefixes = new TreeSet<>(prefixLengthComparator);
        for (RouteInfo route : routes) {
            if (route.getType() != 7) {
                IpPrefix destination = route.getDestination();
                if (destination.isIPv4()) {
                    ipv4Prefixes.add(destination);
                } else {
                    ipv6Prefixes.add(destination);
                }
            }
        }
        return NetworkUtils.routedIPv4AddressCount(ipv4Prefixes) > MOST_IPV4_ADDRESSES_COUNT || NetworkUtils.routedIPv6AddressCount(ipv6Prefixes).compareTo(MOST_IPV6_ADDRESSES_COUNT) >= 0;
    }

    private boolean updateLinkPropertiesInPlaceIfPossible(NetworkAgent agent, VpnConfig oldConfig) {
        if (oldConfig.allowBypass != this.mConfig.allowBypass) {
            Log.i(TAG, "Handover not possible due to changes to allowBypass");
            return false;
        }
        if (!Objects.equals(oldConfig.allowedApplications, this.mConfig.allowedApplications) || !Objects.equals(oldConfig.disallowedApplications, this.mConfig.disallowedApplications)) {
            Log.i(TAG, "Handover not possible due to changes to whitelisted/blacklisted apps");
            return false;
        }
        agent.sendLinkProperties(makeLinkProperties());
        return true;
    }

    /* JADX INFO: Access modifiers changed from: private */
    public void agentConnect() {
        LinkProperties lp = makeLinkProperties();
        this.mNetworkCapabilities.addCapability(12);
        this.mNetworkInfo.setDetailedState(NetworkInfo.DetailedState.CONNECTING, null, null);
        NetworkMisc networkMisc = new NetworkMisc();
        networkMisc.allowBypass = this.mConfig.allowBypass && !this.mLockdown;
        this.mNetworkCapabilities.setEstablishingVpnAppUid(Binder.getCallingUid());
        this.mNetworkCapabilities.setUids(createUserAndRestrictedProfilesRanges(this.mUserHandle, this.mConfig.allowedApplications, this.mConfig.disallowedApplications));
        long token = Binder.clearCallingIdentity();
        try {
            this.mNetworkAgent = new NetworkAgent(this.mLooper, this.mContext, NETWORKTYPE, this.mNetworkInfo, this.mNetworkCapabilities, lp, 101, networkMisc, -2) { // from class: com.android.server.connectivity.Vpn.1
                public void unwanted() {
                }
            };
            Binder.restoreCallingIdentity(token);
            this.mNetworkInfo.setIsAvailable(true);
            updateState(NetworkInfo.DetailedState.CONNECTED, "agentConnect");
        } catch (Throwable th) {
            Binder.restoreCallingIdentity(token);
            throw th;
        }
    }

    private boolean canHaveRestrictedProfile(int userId) {
        long token = Binder.clearCallingIdentity();
        try {
            return UserManager.get(this.mContext).canHaveRestrictedProfile(userId);
        } finally {
            Binder.restoreCallingIdentity(token);
        }
    }

    private void agentDisconnect(NetworkAgent networkAgent) {
        if (networkAgent != null) {
            NetworkInfo networkInfo = new NetworkInfo(this.mNetworkInfo);
            networkInfo.setIsAvailable(false);
            networkInfo.setDetailedState(NetworkInfo.DetailedState.DISCONNECTED, null, null);
            networkAgent.sendNetworkInfo(networkInfo);
        }
    }

    /* JADX INFO: Access modifiers changed from: private */
    public void agentDisconnect() {
        if (this.mNetworkInfo.isConnected()) {
            this.mNetworkInfo.setIsAvailable(false);
            updateState(NetworkInfo.DetailedState.DISCONNECTED, "agentDisconnect");
            this.mNetworkAgent = null;
        }
    }

    public synchronized ParcelFileDescriptor establish(VpnConfig config) {
        UserManager mgr = UserManager.get(this.mContext);
        if (Binder.getCallingUid() != this.mOwnerUID) {
            return null;
        }
        if (!isVpnUserPreConsented(this.mPackage)) {
            return null;
        }
        try {
            Intent intent = new Intent("android.net.VpnService");
            intent.setClassName(this.mPackage, config.user);
            long token = Binder.clearCallingIdentity();
            try {
                UserInfo user = mgr.getUserInfo(this.mUserHandle);
                try {
                    if (user.isRestricted()) {
                        throw new SecurityException("Restricted users cannot establish VPNs");
                    }
                    ResolveInfo info = AppGlobals.getPackageManager().resolveService(intent, (String) null, 0, this.mUserHandle);
                    if (info == null) {
                        throw new SecurityException("Cannot find " + config.user);
                    }
                    if (!"android.permission.BIND_VPN_SERVICE".equals(info.serviceInfo.permission)) {
                        throw new SecurityException(config.user + " does not require android.permission.BIND_VPN_SERVICE");
                    }
                    Binder.restoreCallingIdentity(token);
                    VpnConfig oldConfig = this.mConfig;
                    String oldInterface = this.mInterface;
                    Connection oldConnection = this.mConnection;
                    NetworkAgent oldNetworkAgent = this.mNetworkAgent;
                    Set<UidRange> oldUsers = this.mNetworkCapabilities.getUids();
                    ParcelFileDescriptor tun = ParcelFileDescriptor.adoptFd(jniCreate(config.mtu));
                    try {
                        String interfaze = jniGetName(tun.getFd());
                        StringBuilder builder = new StringBuilder();
                        Iterator it = config.addresses.iterator();
                        while (it.hasNext()) {
                            try {
                                LinkAddress address = (LinkAddress) it.next();
                                StringBuilder sb = new StringBuilder();
                                Iterator it2 = it;
                                sb.append(" ");
                                sb.append(address);
                                builder.append(sb.toString());
                                it = it2;
                            } catch (RuntimeException e) {
                                e = e;
                            }
                        }
                        try {
                            if (jniSetAddresses(interfaze, builder.toString()) < 1) {
                                throw new IllegalArgumentException("At least one address must be specified");
                            }
                            Connection connection = new Connection();
                            try {
                                if (!this.mContext.bindServiceAsUser(intent, connection, 67108865, new UserHandle(this.mUserHandle))) {
                                    throw new IllegalStateException("Cannot bind " + config.user);
                                }
                                this.mConnection = connection;
                                this.mInterface = interfaze;
                                config.user = this.mPackage;
                                config.interfaze = this.mInterface;
                                config.startTime = SystemClock.elapsedRealtime();
                                this.mConfig = config;
                                if (oldConfig == null || !updateLinkPropertiesInPlaceIfPossible(this.mNetworkAgent, oldConfig)) {
                                    this.mNetworkAgent = null;
                                    updateState(NetworkInfo.DetailedState.CONNECTING, "establish");
                                    agentConnect();
                                    agentDisconnect(oldNetworkAgent);
                                }
                                if (oldConnection != null) {
                                    this.mContext.unbindService(oldConnection);
                                }
                                if (oldInterface != null && !oldInterface.equals(interfaze)) {
                                    jniReset(oldInterface);
                                }
                                try {
                                    IoUtils.setBlocking(tun.getFileDescriptor(), config.blocking);
                                    Log.i(TAG, "Established by " + config.user + " on " + this.mInterface);
                                    return tun;
                                } catch (IOException e2) {
                                    throw new IllegalStateException("Cannot set tunnel's fd as blocking=" + config.blocking, e2);
                                }
                            } catch (RuntimeException e3) {
                                e = e3;
                            }
                        } catch (RuntimeException e4) {
                            e = e4;
                        }
                    } catch (RuntimeException e5) {
                        e = e5;
                    }
                    IoUtils.closeQuietly(tun);
                    agentDisconnect();
                    this.mConfig = oldConfig;
                    this.mConnection = oldConnection;
                    this.mNetworkCapabilities.setUids(oldUsers);
                    this.mNetworkAgent = oldNetworkAgent;
                    this.mInterface = oldInterface;
                    throw e;
                } catch (RemoteException e6) {
                }
            } catch (RemoteException e7) {
            } catch (Throwable th) {
                e = th;
                Binder.restoreCallingIdentity(token);
                throw e;
            }
            throw new SecurityException("Cannot find " + config.user);
        } catch (Throwable th2) {
            e = th2;
        }
    }

    private boolean isRunningLocked() {
        return (this.mNetworkAgent == null || this.mInterface == null) ? false : true;
    }

    @VisibleForTesting
    protected boolean isCallerEstablishedOwnerLocked() {
        return isRunningLocked() && Binder.getCallingUid() == this.mOwnerUID;
    }

    private SortedSet<Integer> getAppsUids(List<String> packageNames, int userHandle) {
        SortedSet<Integer> uids = new TreeSet<>();
        for (String app : packageNames) {
            int uid = getAppUid(app, userHandle);
            if (uid != -1) {
                uids.add(Integer.valueOf(uid));
            }
        }
        return uids;
    }

    @VisibleForTesting
    Set<UidRange> createUserAndRestrictedProfilesRanges(int userHandle, List<String> allowedApplications, List<String> disallowedApplications) {
        Set<UidRange> ranges = new ArraySet<>();
        addUserToRanges(ranges, userHandle, allowedApplications, disallowedApplications);
        if (canHaveRestrictedProfile(userHandle)) {
            long token = Binder.clearCallingIdentity();
            try {
                List<UserInfo> users = UserManager.get(this.mContext).getUsers(true);
                Binder.restoreCallingIdentity(token);
                for (UserInfo user : users) {
                    if (user.isRestricted() && user.restrictedProfileParentId == userHandle) {
                        addUserToRanges(ranges, user.id, allowedApplications, disallowedApplications);
                    }
                }
            } catch (Throwable th) {
                Binder.restoreCallingIdentity(token);
                throw th;
            }
        }
        return ranges;
    }

    @VisibleForTesting
    void addUserToRanges(Set<UidRange> ranges, int userHandle, List<String> allowedApplications, List<String> disallowedApplications) {
        if (allowedApplications != null) {
            int start = -1;
            int stop = -1;
            Iterator<Integer> it = getAppsUids(allowedApplications, userHandle).iterator();
            while (it.hasNext()) {
                int uid = it.next().intValue();
                if (start == -1) {
                    start = uid;
                } else if (uid != stop + 1) {
                    ranges.add(new UidRange(start, stop));
                    start = uid;
                }
                stop = uid;
            }
            if (start != -1) {
                ranges.add(new UidRange(start, stop));
                return;
            }
            return;
        }
        if (disallowedApplications != null) {
            UidRange userRange = UidRange.createForUser(userHandle);
            int start2 = userRange.start;
            Iterator<Integer> it2 = getAppsUids(disallowedApplications, userHandle).iterator();
            while (it2.hasNext()) {
                int uid2 = it2.next().intValue();
                if (uid2 == start2) {
                    start2++;
                } else {
                    ranges.add(new UidRange(start2, uid2 - 1));
                    start2 = uid2 + 1;
                }
            }
            if (start2 <= userRange.stop) {
                ranges.add(new UidRange(start2, userRange.stop));
                return;
            }
            return;
        }
        ranges.add(UidRange.createForUser(userHandle));
    }

    private static List<UidRange> uidRangesForUser(int userHandle, Set<UidRange> existingRanges) {
        UidRange userRange = UidRange.createForUser(userHandle);
        List<UidRange> ranges = new ArrayList<>();
        for (UidRange range : existingRanges) {
            if (userRange.containsRange(range)) {
                ranges.add(range);
            }
        }
        return ranges;
    }

    public void onUserAdded(int userHandle) {
        UserInfo user = UserManager.get(this.mContext).getUserInfo(userHandle);
        if (user.isRestricted() && user.restrictedProfileParentId == this.mUserHandle) {
            synchronized (this) {
                Set existingRanges = this.mNetworkCapabilities.getUids();
                if (existingRanges != null) {
                    try {
                        addUserToRanges(existingRanges, userHandle, this.mConfig.allowedApplications, this.mConfig.disallowedApplications);
                        this.mNetworkCapabilities.setUids(existingRanges);
                    } catch (Exception e) {
                        Log.wtf(TAG, "Failed to add restricted user to owner", e);
                    }
                    setVpnForcedLocked(this.mLockdown);
                } else {
                    setVpnForcedLocked(this.mLockdown);
                }
                throw th;
            }
        }
    }

    public void onUserRemoved(int userHandle) {
        UserInfo user = UserManager.get(this.mContext).getUserInfo(userHandle);
        if (user.isRestricted() && user.restrictedProfileParentId == this.mUserHandle) {
            synchronized (this) {
                Set<UidRange> existingRanges = this.mNetworkCapabilities.getUids();
                if (existingRanges != null) {
                    try {
                        List<UidRange> removedRanges = uidRangesForUser(userHandle, existingRanges);
                        existingRanges.removeAll(removedRanges);
                        this.mNetworkCapabilities.setUids(existingRanges);
                    } catch (Exception e) {
                        Log.wtf(TAG, "Failed to remove restricted user to owner", e);
                    }
                    setVpnForcedLocked(this.mLockdown);
                } else {
                    setVpnForcedLocked(this.mLockdown);
                }
                throw th;
            }
        }
    }

    public synchronized void onUserStopped() {
        setLockdown(false);
        this.mAlwaysOn = false;
        agentDisconnect();
    }

    @GuardedBy({"this"})
    private void setVpnForcedLocked(boolean enforce) {
        List<String> exemptedPackages;
        if (isNullOrLegacyVpn(this.mPackage)) {
            exemptedPackages = null;
        } else {
            exemptedPackages = new ArrayList<>(this.mLockdownWhitelist);
            exemptedPackages.add(this.mPackage);
        }
        Set<UidRange> removedRanges = new ArraySet<>(this.mBlockedUsers);
        Set<UidRange> addedRanges = Collections.emptySet();
        if (enforce) {
            addedRanges = createUserAndRestrictedProfilesRanges(this.mUserHandle, null, exemptedPackages);
            for (UidRange range : addedRanges) {
                if (range.start == 0) {
                    addedRanges.remove(range);
                    if (range.stop != 0) {
                        addedRanges.add(new UidRange(1, range.stop));
                    }
                }
            }
            removedRanges.removeAll(addedRanges);
            addedRanges.removeAll(this.mBlockedUsers);
        }
        setAllowOnlyVpnForUids(false, removedRanges);
        setAllowOnlyVpnForUids(true, addedRanges);
    }

    @GuardedBy({"this"})
    private boolean setAllowOnlyVpnForUids(boolean enforce, Collection<UidRange> ranges) {
        if (ranges.size() == 0) {
            return true;
        }
        UidRange[] rangesArray = (UidRange[]) ranges.toArray(new UidRange[ranges.size()]);
        try {
            this.mNetd.setAllowOnlyVpnForUids(enforce, rangesArray);
            if (enforce) {
                this.mBlockedUsers.addAll(ranges);
            } else {
                this.mBlockedUsers.removeAll(ranges);
            }
            return true;
        } catch (RemoteException | RuntimeException e) {
            Log.e(TAG, "Updating blocked=" + enforce + " for UIDs " + Arrays.toString(ranges.toArray()) + " failed", e);
            return false;
        }
    }

    public VpnConfig getVpnConfig() {
        enforceControlPermission();
        return this.mConfig;
    }

    @Deprecated
    public synchronized void interfaceStatusChanged(String iface, boolean up) {
        try {
            try {
                this.mObserver.interfaceStatusChanged(iface, up);
            } catch (RemoteException e) {
            }
        } catch (RemoteException e2) {
        }
    }

    private void enforceControlPermission() {
        this.mContext.enforceCallingPermission("android.permission.CONTROL_VPN", "Unauthorized Caller");
    }

    private void enforceControlPermissionOrInternalCaller() {
        this.mContext.enforceCallingOrSelfPermission("android.permission.CONTROL_VPN", "Unauthorized Caller");
    }

    private void enforceSettingsPermission() {
        this.mContext.enforceCallingOrSelfPermission("android.permission.NETWORK_SETTINGS", "Unauthorized Caller");
    }

    private class Connection implements ServiceConnection {
        private IBinder mService;

        private Connection() {
        }

        @Override // android.content.ServiceConnection
        public void onServiceConnected(ComponentName name, IBinder service) {
            this.mService = service;
        }

        @Override // android.content.ServiceConnection
        public void onServiceDisconnected(ComponentName name) {
            this.mService = null;
        }
    }

    /* JADX INFO: Access modifiers changed from: private */
    public void prepareStatusIntent() {
        long token = Binder.clearCallingIdentity();
        try {
            this.mStatusIntent = VpnConfig.getIntentForStatusPanel(this.mContext);
        } finally {
            Binder.restoreCallingIdentity(token);
        }
    }

    public synchronized boolean addAddress(String address, int prefixLength) {
        if (!isCallerEstablishedOwnerLocked()) {
            return false;
        }
        boolean success = jniAddAddress(this.mInterface, address, prefixLength);
        this.mNetworkAgent.sendLinkProperties(makeLinkProperties());
        return success;
    }

    public synchronized boolean removeAddress(String address, int prefixLength) {
        if (!isCallerEstablishedOwnerLocked()) {
            return false;
        }
        boolean success = jniDelAddress(this.mInterface, address, prefixLength);
        this.mNetworkAgent.sendLinkProperties(makeLinkProperties());
        return success;
    }

    public synchronized boolean setUnderlyingNetworks(Network[] networks) {
        if (!isCallerEstablishedOwnerLocked()) {
            return false;
        }
        if (networks == null) {
            this.mConfig.underlyingNetworks = null;
        } else {
            this.mConfig.underlyingNetworks = new Network[networks.length];
            for (int i = 0; i < networks.length; i++) {
                if (networks[i] == null) {
                    this.mConfig.underlyingNetworks[i] = null;
                } else {
                    this.mConfig.underlyingNetworks[i] = new Network(networks[i].netId);
                }
            }
        }
        return true;
    }

    public synchronized Network[] getUnderlyingNetworks() {
        if (!isRunningLocked()) {
            return null;
        }
        return this.mConfig.underlyingNetworks;
    }

    public synchronized VpnInfo getVpnInfo() {
        if (!isRunningLocked()) {
            return null;
        }
        VpnInfo info = new VpnInfo();
        info.ownerUid = this.mOwnerUID;
        info.vpnIface = this.mInterface;
        return info;
    }

    public synchronized boolean appliesToUid(int uid) {
        if (!isRunningLocked()) {
            return false;
        }
        return this.mNetworkCapabilities.appliesToUid(uid);
    }

    public synchronized boolean isBlockingUid(int uid) {
        if (this.mNetworkInfo.isConnected()) {
            return !appliesToUid(uid);
        }
        return UidRange.containsUid(this.mBlockedUsers, uid);
    }

    private void updateAlwaysOnNotification(NetworkInfo.DetailedState networkState) {
        boolean visible = this.mAlwaysOn && networkState != NetworkInfo.DetailedState.CONNECTED;
        UserHandle user = UserHandle.of(this.mUserHandle);
        long token = Binder.clearCallingIdentity();
        try {
            NotificationManager notificationManager = NotificationManager.from(this.mContext);
            if (!visible) {
                notificationManager.cancelAsUser(TAG, 17, user);
                return;
            }
            Intent intent = new Intent();
            intent.setComponent(ComponentName.unflattenFromString(this.mContext.getString(R.string.android_start_title)));
            intent.putExtra("lockdown", this.mLockdown);
            intent.addFlags(268435456);
            PendingIntent configIntent = this.mSystemServices.pendingIntentGetActivityAsUser(intent, 201326592, user);
            Notification.Builder builder = new Notification.Builder(this.mContext, SystemNotificationChannels.VPN).setSmallIcon(R.drawable.rate_star_big_half_holo_dark).setContentTitle(this.mContext.getString(R.string.permdesc_foregroundServiceMicrophone)).setContentText(this.mContext.getString(R.string.permdesc_foregroundServiceMediaPlayback)).setContentIntent(configIntent).setCategory("sys").setVisibility(1).setOngoing(true).setColor(this.mContext.getColor(R.color.system_notification_accent_color));
            notificationManager.notifyAsUser(TAG, 17, builder.build(), user);
        } finally {
            Binder.restoreCallingIdentity(token);
        }
    }

    @VisibleForTesting
    public static class SystemServices {
        private final Context mContext;

        public SystemServices(Context context) {
            this.mContext = context;
        }

        public PendingIntent pendingIntentGetActivityAsUser(Intent intent, int flags, UserHandle user) {
            return PendingIntent.getActivityAsUser(this.mContext, 0, intent, flags, null, user);
        }

        public void settingsSecurePutStringForUser(String key, String value, int userId) {
            Settings.Secure.putStringForUser(this.mContext.getContentResolver(), key, value, userId);
        }

        public void settingsSecurePutIntForUser(String key, int value, int userId) {
            Settings.Secure.putIntForUser(this.mContext.getContentResolver(), key, value, userId);
        }

        public String settingsSecureGetStringForUser(String key, int userId) {
            return Settings.Secure.getStringForUser(this.mContext.getContentResolver(), key, userId);
        }

        public int settingsSecureGetIntForUser(String key, int def, int userId) {
            return Settings.Secure.getIntForUser(this.mContext.getContentResolver(), key, def, userId);
        }
    }

    private static RouteInfo findIPv4DefaultRoute(LinkProperties prop) {
        for (RouteInfo route : prop.getAllRoutes()) {
            if (route.isDefaultRoute() && (route.getGateway() instanceof Inet4Address)) {
                return route;
            }
        }
        throw new IllegalStateException("Unable to find IPv4 default gateway");
    }

    public void startLegacyVpn(VpnProfile profile, KeyStore keyStore, LinkProperties egress) {
        enforceControlPermission();
        long token = Binder.clearCallingIdentity();
        try {
            startLegacyVpnPrivileged(profile, keyStore, egress);
        } finally {
            Binder.restoreCallingIdentity(token);
        }
    }

    public void startLegacyVpnPrivileged(VpnProfile profile, KeyStore keyStore, LinkProperties egress) {
        UserManager mgr = UserManager.get(this.mContext);
        UserInfo user = mgr.getUserInfo(this.mUserHandle);
        if (user.isRestricted() || mgr.hasUserRestriction("no_config_vpn", new UserHandle(this.mUserHandle))) {
            throw new SecurityException("Restricted users cannot establish VPNs");
        }
        RouteInfo ipv4DefaultRoute = findIPv4DefaultRoute(egress);
        String gateway = ipv4DefaultRoute.getGateway().getHostAddress();
        String iface = ipv4DefaultRoute.getInterface();
        String privateKey = "";
        String userCert = "";
        String caCert = "";
        String serverCert = "";
        if (!profile.ipsecUserCert.isEmpty()) {
            privateKey = "USRPKEY_" + profile.ipsecUserCert;
            byte[] value = keyStore.get("USRCERT_" + profile.ipsecUserCert);
            userCert = value == null ? null : new String(value, StandardCharsets.UTF_8);
        }
        if (!profile.ipsecCaCert.isEmpty()) {
            byte[] value2 = keyStore.get("CACERT_" + profile.ipsecCaCert);
            caCert = value2 == null ? null : new String(value2, StandardCharsets.UTF_8);
        }
        if (!profile.ipsecServerCert.isEmpty()) {
            byte[] value3 = keyStore.get("USRCERT_" + profile.ipsecServerCert);
            serverCert = value3 != null ? new String(value3, StandardCharsets.UTF_8) : null;
        }
        if (privateKey == null || userCert == null || caCert == null || serverCert == null) {
            throw new IllegalStateException("Cannot load credentials");
        }
        String[] racoon = null;
        int i = profile.type;
        if (i == 1) {
            racoon = new String[]{iface, profile.server, "udppsk", profile.ipsecIdentifier, profile.ipsecSecret, "1701"};
        } else if (i == 2) {
            racoon = new String[]{iface, profile.server, "udprsa", privateKey, userCert, caCert, serverCert, "1701"};
        } else if (i == 3) {
            racoon = new String[]{iface, profile.server, "xauthpsk", profile.ipsecIdentifier, profile.ipsecSecret, profile.username, profile.password, "", gateway};
        } else if (i == 4) {
            racoon = new String[]{iface, profile.server, "xauthrsa", privateKey, userCert, caCert, serverCert, profile.username, profile.password, "", gateway};
        } else if (i == 5) {
            racoon = new String[]{iface, profile.server, "hybridrsa", caCert, serverCert, profile.username, profile.password, "", gateway};
        }
        String[] mtpd = null;
        int i2 = profile.type;
        if (i2 == 0) {
            String[] strArr = new String[20];
            strArr[0] = iface;
            strArr[1] = "pptp";
            strArr[2] = profile.server;
            strArr[3] = "1723";
            strArr[4] = com.android.server.pm.Settings.ATTR_NAME;
            strArr[5] = profile.username;
            strArr[6] = "password";
            strArr[7] = profile.password;
            strArr[8] = "linkname";
            strArr[9] = "vpn";
            strArr[10] = "refuse-eap";
            strArr[11] = "nodefaultroute";
            strArr[12] = "usepeerdns";
            strArr[13] = "idle";
            strArr[14] = "1800";
            strArr[15] = "mtu";
            strArr[16] = "1400";
            strArr[17] = "mru";
            strArr[18] = "1400";
            strArr[19] = profile.mppe ? "+mppe" : "nomppe";
            mtpd = strArr;
        } else if (i2 == 1 || i2 == 2) {
            mtpd = new String[]{iface, "l2tp", profile.server, "1701", profile.l2tpSecret, com.android.server.pm.Settings.ATTR_NAME, profile.username, "password", profile.password, "linkname", "vpn", "refuse-eap", "nodefaultroute", "usepeerdns", "idle", "1800", "mtu", "1400", "mru", "1400"};
        }
        VpnConfig config = new VpnConfig();
        config.legacy = true;
        config.user = profile.key;
        config.interfaze = iface;
        config.session = profile.name;
        config.isMetered = false;
        config.proxyInfo = profile.proxy;
        config.addLegacyRoutes(profile.routes);
        if (!profile.dnsServers.isEmpty()) {
            config.dnsServers = Arrays.asList(profile.dnsServers.split(" +"));
        }
        if (!profile.searchDomains.isEmpty()) {
            config.searchDomains = Arrays.asList(profile.searchDomains.split(" +"));
        }
        startLegacyVpn(config, racoon, mtpd, profile);
    }

    private synchronized void startLegacyVpn(VpnConfig config, String[] racoon, String[] mtpd, VpnProfile profile) {
        stopLegacyVpnPrivileged();
        prepareInternal("[Legacy VPN]");
        updateState(NetworkInfo.DetailedState.CONNECTING, "startLegacyVpn");
        this.mLegacyVpnRunner = new LegacyVpnRunner(config, racoon, mtpd, profile);
        this.mLegacyVpnRunner.start();
    }

    /* JADX WARN: Bottom block not found for handler: all -> 0x0015 */
    /*
        Code decompiled incorrectly, please refer to instructions dump.
        To view partially-correct add '--show-bad-code' argument
    */
    public synchronized void stopLegacyVpnPrivileged() {
        /*
            r2 = this;
            monitor-enter(r2)
            com.android.server.connectivity.Vpn$LegacyVpnRunner r0 = r2.mLegacyVpnRunner     // Catch: java.lang.Throwable -> L19
            if (r0 == 0) goto L17
            com.android.server.connectivity.Vpn$LegacyVpnRunner r0 = r2.mLegacyVpnRunner     // Catch: java.lang.Throwable -> L19
            r0.exit()     // Catch: java.lang.Throwable -> L19
            r0 = 0
            r2.mLegacyVpnRunner = r0     // Catch: java.lang.Throwable -> L19
            java.lang.String r0 = "LegacyVpnRunner"
            monitor-enter(r0)     // Catch: java.lang.Throwable -> L19
            monitor-exit(r0)     // Catch: java.lang.Throwable -> L12
            goto L17
        L12:
            r1 = move-exception
        L13:
            monitor-exit(r0)     // Catch: java.lang.Throwable -> L15
            throw r1     // Catch: java.lang.Throwable -> L19
        L15:
            r1 = move-exception
            goto L13
        L17:
            monitor-exit(r2)
            return
        L19:
            r0 = move-exception
            monitor-exit(r2)
            throw r0
        */
        throw new UnsupportedOperationException("Method not decompiled: com.android.server.connectivity.Vpn.stopLegacyVpnPrivileged():void");
    }

    public synchronized LegacyVpnInfo getLegacyVpnInfo() {
        enforceControlPermission();
        return getLegacyVpnInfoPrivileged();
    }

    public synchronized LegacyVpnInfo getLegacyVpnInfoPrivileged() {
        if (this.mLegacyVpnRunner == null) {
            return null;
        }
        LegacyVpnInfo info = new LegacyVpnInfo();
        info.key = this.mConfig.user;
        info.state = LegacyVpnInfo.stateFromNetworkInfo(this.mNetworkInfo);
        if (this.mNetworkInfo.isConnected()) {
            info.intent = this.mStatusIntent;
        }
        return info;
    }

    public VpnConfig getLegacyVpnConfig() {
        if (this.mLegacyVpnRunner != null) {
            return this.mConfig;
        }
        return null;
    }

    private class LegacyVpnRunner extends Thread {
        private static final String TAG = "LegacyVpnRunner";
        private final String[][] mArguments;
        private long mBringupStartTime;
        private final BroadcastReceiver mBroadcastReceiver;
        private final String[] mDaemons;
        private final AtomicInteger mOuterConnection;
        private final String mOuterInterface;
        private final VpnProfile mProfile;
        private final LocalSocket[] mSockets;

        LegacyVpnRunner(VpnConfig config, String[] racoon, String[] mtpd, VpnProfile profile) {
            NetworkInfo networkInfo;
            super(TAG);
            this.mOuterConnection = new AtomicInteger(-1);
            this.mBringupStartTime = -1L;
            this.mBroadcastReceiver = new BroadcastReceiver() { // from class: com.android.server.connectivity.Vpn.LegacyVpnRunner.1
                @Override // android.content.BroadcastReceiver
                public void onReceive(Context context, Intent intent) {
                    NetworkInfo info;
                    if (Vpn.this.mEnableTeardown && intent.getAction().equals("android.net.conn.CONNECTIVITY_CHANGE") && intent.getIntExtra("networkType", -1) == LegacyVpnRunner.this.mOuterConnection.get() && (info = (NetworkInfo) intent.getExtra("networkInfo")) != null && !info.isConnectedOrConnecting()) {
                        try {
                            Vpn.this.mObserver.interfaceStatusChanged(LegacyVpnRunner.this.mOuterInterface, false);
                        } catch (RemoteException e) {
                        }
                    }
                }
            };
            Vpn.this.mConfig = config;
            this.mDaemons = new String[]{"racoon", "mtpd"};
            this.mArguments = new String[][]{racoon, mtpd};
            this.mSockets = new LocalSocket[this.mDaemons.length];
            this.mOuterInterface = Vpn.this.mConfig.interfaze;
            this.mProfile = profile;
            if (!TextUtils.isEmpty(this.mOuterInterface)) {
                ConnectivityManager cm = ConnectivityManager.from(Vpn.this.mContext);
                for (Network network : cm.getAllNetworks()) {
                    LinkProperties lp = cm.getLinkProperties(network);
                    if (lp != null && lp.getAllInterfaceNames().contains(this.mOuterInterface) && (networkInfo = cm.getNetworkInfo(network)) != null) {
                        this.mOuterConnection.set(networkInfo.getType());
                    }
                }
            }
            IntentFilter filter = new IntentFilter();
            filter.addAction("android.net.conn.CONNECTIVITY_CHANGE");
            Vpn.this.mContext.registerReceiver(this.mBroadcastReceiver, filter);
        }

        public void check(String interfaze) {
            if (interfaze.equals(this.mOuterInterface)) {
                Log.i(TAG, "Legacy VPN is going down with " + interfaze);
                exit();
            }
        }

        public void exit() {
            interrupt();
            Vpn.this.agentDisconnect();
            try {
                Vpn.this.mContext.unregisterReceiver(this.mBroadcastReceiver);
            } catch (IllegalArgumentException e) {
            }
        }

        @Override // java.lang.Thread, java.lang.Runnable
        public void run() {
            Log.v(TAG, "Waiting");
            synchronized (TAG) {
                try {
                    Log.v(TAG, "Executing");
                    int i = 0;
                    try {
                        bringup();
                        waitForDaemonsToStop();
                        interrupted();
                    } catch (InterruptedException e) {
                    } finally {
                        for (LocalSocket socket : this.mSockets) {
                            IoUtils.closeQuietly(socket);
                        }
                        try {
                            Thread.sleep(50L);
                        } catch (InterruptedException e2) {
                        }
                        String[] strArr = this.mDaemons;
                        int length = strArr.length;
                        while (i < length) {
                            String daemon = strArr[i];
                            SystemService.stop(daemon);
                            i++;
                        }
                    }
                    Vpn.this.agentDisconnect();
                } catch (Throwable th) {
                    throw th;
                }
            }
        }

        private void checkInterruptAndDelay(boolean sleepLonger) throws InterruptedException {
            long now = SystemClock.elapsedRealtime();
            if (now - this.mBringupStartTime <= 60000) {
                Thread.sleep(sleepLonger ? 200L : 1L);
            } else {
                Vpn.this.updateState(NetworkInfo.DetailedState.FAILED, "checkpoint");
                throw new IllegalStateException("VPN bringup took too long");
            }
        }

        private void bringup() {
            boolean z;
            String str;
            try {
                this.mBringupStartTime = SystemClock.elapsedRealtime();
                String[] strArr = this.mDaemons;
                int length = strArr.length;
                int i = 0;
                while (true) {
                    z = true;
                    if (i >= length) {
                        break;
                    }
                    String daemon = strArr[i];
                    while (!SystemService.isStopped(daemon)) {
                        checkInterruptAndDelay(true);
                    }
                    i++;
                }
                File state = new File("/data/misc/vpn/state");
                state.delete();
                if (state.exists()) {
                    throw new IllegalStateException("Cannot delete the state");
                }
                new File("/data/misc/vpn/abort").delete();
                boolean restart = false;
                for (String[] strArr2 : this.mArguments) {
                    restart = restart || strArr2 != null;
                }
                if (!restart) {
                    Vpn.this.agentDisconnect();
                    return;
                }
                Vpn.this.updateState(NetworkInfo.DetailedState.CONNECTING, "execute");
                int i2 = 0;
                while (i2 < this.mDaemons.length) {
                    String[] arguments = this.mArguments[i2];
                    if (arguments != null) {
                        String daemon2 = this.mDaemons[i2];
                        SystemService.start(daemon2);
                        while (!SystemService.isRunning(daemon2)) {
                            checkInterruptAndDelay(z);
                        }
                        this.mSockets[i2] = new LocalSocket();
                        LocalSocketAddress address = new LocalSocketAddress(daemon2, LocalSocketAddress.Namespace.RESERVED);
                        while (true) {
                            try {
                                this.mSockets[i2].connect(address);
                                break;
                            } catch (Exception e) {
                                checkInterruptAndDelay(true);
                            }
                        }
                        this.mSockets[i2].setSoTimeout(com.android.server.SystemService.PHASE_SYSTEM_SERVICES_READY);
                        OutputStream out = this.mSockets[i2].getOutputStream();
                        for (String argument : arguments) {
                            byte[] bytes = argument.getBytes(StandardCharsets.UTF_8);
                            if (bytes.length >= 65535) {
                                throw new IllegalArgumentException("Argument is too large");
                            }
                            out.write(bytes.length >> 8);
                            out.write(bytes.length);
                            out.write(bytes);
                            checkInterruptAndDelay(false);
                        }
                        out.write(255);
                        out.write(255);
                        out.flush();
                        InputStream in = this.mSockets[i2].getInputStream();
                        while (in.read() != -1) {
                            checkInterruptAndDelay(true);
                        }
                    }
                    i2++;
                    z = true;
                }
                while (!state.exists()) {
                    for (int i3 = 0; i3 < this.mDaemons.length; i3++) {
                        String daemon3 = this.mDaemons[i3];
                        if (this.mArguments[i3] != null && !SystemService.isRunning(daemon3)) {
                            throw new IllegalStateException(daemon3 + " is dead");
                        }
                    }
                    checkInterruptAndDelay(true);
                }
                String[] parameters = FileUtils.readTextFile(state, 0, null).split("\n", -1);
                if (parameters.length != 7) {
                    throw new IllegalStateException("Cannot parse the state");
                }
                Vpn.this.mConfig.interfaze = parameters[0].trim();
                Vpn.this.mConfig.addLegacyAddresses(parameters[1]);
                if (Vpn.this.mConfig.routes == null || Vpn.this.mConfig.routes.isEmpty()) {
                    Vpn.this.mConfig.addLegacyRoutes(parameters[2]);
                }
                if (Vpn.this.mConfig.dnsServers == null || Vpn.this.mConfig.dnsServers.size() == 0) {
                    String dnsServers = parameters[3].trim();
                    if (!dnsServers.isEmpty()) {
                        Vpn.this.mConfig.dnsServers = Arrays.asList(dnsServers.split(" "));
                    }
                }
                if (Vpn.this.mConfig.searchDomains == null || Vpn.this.mConfig.searchDomains.size() == 0) {
                    String searchDomains = parameters[4].trim();
                    if (!searchDomains.isEmpty()) {
                        Vpn.this.mConfig.searchDomains = Arrays.asList(searchDomains.split(" "));
                    }
                }
                if (parameters[5].isEmpty()) {
                    str = this.mProfile.server;
                } else {
                    str = parameters[5];
                }
                String endpoint = str;
                if (!InetAddresses.isNumericAddress(endpoint)) {
                    Log.i(TAG, "VPN Endpoint is not numeric. Converting to numeric");
                    try {
                        endpoint = InetAddress.getByName(endpoint).getHostAddress();
                    } catch (UnknownHostException e2) {
                        Log.e(TAG, "Can't parse InetAddress from string: unknown host.");
                        return;
                    }
                }
                if (!endpoint.isEmpty()) {
                    try {
                        InetAddress addr = InetAddress.parseNumericAddress(endpoint);
                        if (addr instanceof Inet4Address) {
                            Vpn.this.mConfig.routes.add(new RouteInfo(new IpPrefix(addr, 32), 9));
                        } else if (addr instanceof Inet6Address) {
                            Vpn.this.mConfig.routes.add(new RouteInfo(new IpPrefix(addr, 128), 9));
                        } else {
                            Log.e(TAG, "Unknown IP address family for VPN endpoint: " + endpoint);
                        }
                    } catch (IllegalArgumentException e3) {
                        Log.e(TAG, "Exception constructing throw route to " + endpoint + ": " + e3);
                    }
                }
                synchronized (Vpn.this) {
                    Vpn.this.mConfig.startTime = SystemClock.elapsedRealtime();
                    checkInterruptAndDelay(false);
                    if (Vpn.this.jniCheck(Vpn.this.mConfig.interfaze) != 0) {
                        Vpn.this.mInterface = Vpn.this.mConfig.interfaze;
                        Vpn.this.prepareStatusIntent();
                        Vpn.this.agentConnect();
                        Log.i(TAG, "Connected!");
                    } else {
                        throw new IllegalStateException(Vpn.this.mConfig.interfaze + " is gone");
                    }
                }
            } catch (Exception e4) {
                Log.i(TAG, "Aborting", e4);
                Vpn.this.updateState(NetworkInfo.DetailedState.FAILED, e4.getMessage());
                exit();
            }
        }

        private void waitForDaemonsToStop() throws InterruptedException {
            if (!Vpn.this.mNetworkInfo.isConnected()) {
                return;
            }
            while (true) {
                Thread.sleep(2000L);
                int i = 0;
                while (true) {
                    String[] strArr = this.mDaemons;
                    if (i < strArr.length) {
                        if (this.mArguments[i] == null || !SystemService.isStopped(strArr[i])) {
                            i++;
                        } else {
                            return;
                        }
                    }
                }
            }
        }
    }
}
