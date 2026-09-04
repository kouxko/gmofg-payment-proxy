import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const repositoryRoot = path.join(import.meta.dirname, "..");
const workflowPath = path.join(repositoryRoot, ".gitlab-ci.yml");

async function readWorkflow() {
  return await readFile(workflowPath, "utf8");
}

async function readRepositoryFile(relativePath) {
  return await readFile(path.join(repositoryRoot, relativePath), "utf8");
}

function topLevelBlock(source, key) {
  const marker = `${key}:\n`;
  const start = source.indexOf(marker);
  assert.notEqual(start, -1, `missing top-level GitLab CI key: ${key}`);
  const bodyStart = start + marker.length;
  const remainder = source.slice(bodyStart);
  const nextKeyOffset = remainder.search(
    /^[A-Za-z_.][A-Za-z0-9_.-]*:\s*$/mu,
  );
  const end = nextKeyOffset === -1 ? source.length : bodyStart + nextKeyOffset;
  return source.slice(start, end);
}

test("GitLab workflow uses the known project pipeline sources without duplicate branch pipelines", async () => {
  const source = await readWorkflow();
  const workflow = topLevelBlock(source, "workflow");

  assert.match(workflow, /^workflow:\n[ ]{2}rules:$/mu);
  assert.match(workflow, /CI_PIPELINE_SOURCE == "merge_request_event"/u);
  assert.match(
    workflow,
    /CI_COMMIT_BRANCH && \$CI_OPEN_MERGE_REQUESTS && \$CI_PIPELINE_SOURCE == "push"/u,
  );
  assert.match(
    workflow,
    /CI_PIPELINE_SOURCE == "push" && \$CI_COMMIT_BRANCH == \$CI_DEFAULT_BRANCH/u,
  );
  assert.match(
    workflow,
    /CI_PIPELINE_SOURCE == "push" && \$CI_COMMIT_TAG/u,
  );
  assert.match(workflow, /CI_PIPELINE_SOURCE == "web"/u);
  assert.match(
    workflow,
    /PIPELINE_MODE == "android-windows-build" && \(\$CI_PIPELINE_SOURCE == "api" \|\| \$CI_PIPELINE_SOURCE == "web"\)/u,
  );
  assert.match(
    workflow,
    /PIPELINE_MODE == "android-windows-build" && \(\$CI_PIPELINE_SOURCE == "api" \|\| \$CI_PIPELINE_SOURCE == "web"\)'\n[ ]{4}- if: '\$PIPELINE_MODE == "android-windows-build"'\n[ ]{6}when: never/u,
  );
  assert.match(
    workflow,
    /CI_OPEN_MERGE_REQUESTS[^]*?CI_PIPELINE_SOURCE == "push"[^]*?when: never/u,
  );
  assert.doesNotMatch(workflow, /CI_PIPELINE_SOURCE == "schedule"/u);
});

test("GitLab jobs preserve Android, coverage, Windows verification and artifact ordering", async () => {
  const source = await readWorkflow();

  for (
    const job of [
      "android_companion",
      "coverage_gates",
      "verify_windows",
      "package_windows_unsigned",
    ]
  ) {
    assert.match(source, new RegExp(`^${job}:$`, "mu"));
  }
  const androidJob = topLevelBlock(source, "android_companion");
  const coverageJob = topLevelBlock(source, "coverage_gates");
  const verifyJob = topLevelBlock(source, "verify_windows");
  const packageJob = topLevelBlock(source, "package_windows_unsigned");

  assert.match(androidJob, /^[ ]{4}- "docker test"$/mu);
  assert.match(coverageJob, /^[ ]{4}- "docker test"$/mu);
  assert.match(verifyJob, /^[ ]{4}- "slave4"$/mu);
  assert.match(packageJob, /^[ ]{4}- "slave4"$/mu);
  for (const job of [androidJob, coverageJob, verifyJob, packageJob]) {
    assert.match(job, /prefix: "[^"]+-\$CI_COMMIT_REF_SLUG"/u);
  }
  for (const job of [coverageJob, verifyJob, packageJob]) {
    assert.match(job, /job: android_companion/u);
    assert.match(job, /artifacts: true/u);
  }
  assert.match(verifyJob, /^verify_windows:\n[ ]{2}stage: verify/mu);
  assert.match(verifyJob, /^[ ]{2}timeout: 2h 30m$/mu);
  assert.match(coverageJob, /deno task check:coverage:frontend/u);
  assert.match(coverageJob, /deno task check:coverage:rust/u);
  assert.match(verifyJob, /stage-android-companion-windows\.ps1/u);
  assert.match(verifyJob, /deno task test:gitlab-ci/u);
  assert.match(verifyJob, /deno task test:deno-toolchain/u);
  assert.match(verifyJob, /deno task scan:architecture/u);
  assert.match(
    verifyJob,
    /cargo clippy --manifest-path src-tauri\/Cargo\.toml/u,
  );
  assert.match(verifyJob, /deno task test:protocol-packages/u);
  assert.match(verifyJob, /test-support\/socket-relay-gate\/Cargo\.toml/u);
});

test("GitLab toolchains are pinned and frontend commands remain Deno-only", async () => {
  const source = await readWorkflow();
  const variables = topLevelBlock(source, "variables");

  assert.match(variables, /^[ ]{2}DENO_VERSION: "2\.9\.6"$/mu);
  assert.match(
    variables,
    /^[ ]{2}DENO_DIST_BASE_URL: "https:\/\/cdn\.npmmirror\.com\/binaries\/deno"$/mu,
  );
  assert.match(
    variables,
    /^[ ]{2}TAURI_BUNDLER_TOOLS_GITHUB_MIRROR: "https:\/\/gh-proxy\.com\/"$/mu,
  );
  assert.match(
    variables,
    /^[ ]{2}VISUAL_STUDIO_BUILD_TOOLS_URL: "https:\/\/aka\.ms\/vs\/17\/release\/vs_buildtools\.exe"$/mu,
  );
  assert.match(
    variables,
    /^[ ]{2}VISUAL_STUDIO_BUILD_TOOLS_INSTALL_PATH: 'C:\\BuildTools'$/mu,
  );
  assert.match(
    variables,
    /^[ ]{2}WINDOWS_PERL_PACKAGE_URL: "https:\/\/mirrors\.tuna\.tsinghua\.edu\.cn\/msys2\/mingw\/mingw64\/mingw-w64-x86_64-perl-5\.38\.5-1-any\.pkg\.tar\.zst"$/mu,
  );
  assert.match(
    variables,
    /^[ ]{2}WINDOWS_PERL_GCC_LIBS_PACKAGE_URL: "https:\/\/mirrors\.tuna\.tsinghua\.edu\.cn\/msys2\/mingw\/mingw64\/mingw-w64-x86_64-gcc-libs-15\.2\.0-14-any\.pkg\.tar\.zst"$/mu,
  );
  assert.match(
    variables,
    /^[ ]{2}WINDOWS_PERL_PTHREAD_PACKAGE_URL: "https:\/\/mirrors\.tuna\.tsinghua\.edu\.cn\/msys2\/mingw\/mingw64\/mingw-w64-x86_64-libwinpthread-git-12\.0\.0\.r747\.g1a99f8514-1-any\.pkg\.tar\.zst"$/mu,
  );
  assert.match(
    variables,
    /^[ ]{2}WINDOWS_PERL_TZDATA_PACKAGE_URL: "https:\/\/mirrors\.tuna\.tsinghua\.edu\.cn\/msys2\/mingw\/mingw64\/mingw-w64-x86_64-tzdata-2026c-1-any\.pkg\.tar\.zst"$/mu,
  );
  assert.match(variables, /^[ ]{2}PIPELINE_MODE: "full"$/mu);
  assert.match(
    variables,
    /^[ ]{2}APT_MIRROR_URL: "http:\/\/repo\.huaweicloud\.com\/ubuntu"$/mu,
  );
  assert.match(
    variables,
    /^[ ]{2}NPM_CONFIG_REGISTRY: "https:\/\/registry\.npmmirror\.com"$/mu,
  );
  assert.match(
    variables,
    /^[ ]{2}RUSTUP_DIST_SERVER: "https:\/\/rsproxy\.cn"$/mu,
  );
  assert.match(
    variables,
    /^[ ]{2}CARGO_RSPROXY_INDEX: "sparse\+https:\/\/rsproxy\.cn\/index\/"$/mu,
  );
  assert.match(variables, /^[ ]{2}ANDROID_MAVEN_MIRROR: "aliyun"$/mu);
  assert.match(
    variables,
    /^[ ]{2}GRADLE_DISTRIBUTION_BASE_URL: "https:\/\/repo\.huaweicloud\.com\/gradle"$/mu,
  );
  assert.match(variables, /^[ ]{2}RUSTUP_VERSION: "1\.29\.1"$/mu);
  assert.match(variables, /^[ ]{2}RUST_TOOLCHAIN: "1\.98\.0"$/mu);
  assert.match(variables, /^[ ]{2}GRADLE_VERSION: "9\.6\.1"$/mu);
  assert.match(variables, /^[ ]{2}ANDROID_NDK_VERSION: "29\.0\.14206865"$/mu);
  assert.match(
    variables,
    /^[ ]{2}ANDROID_BUILD_TOOLS_ARCHIVE_URL: "https:\/\/mirrors\.cloud\.tencent\.com\/AndroidSDK\/build-tools_r36_linux\.zip"$/mu,
  );
  assert.match(
    variables,
    /^[ ]{2}ANDROID_PLATFORM_ARCHIVE_URL: "https:\/\/mirrors\.cloud\.tencent\.com\/AndroidSDK\/platform-36_r02\.zip"$/mu,
  );
  assert.match(
    variables,
    /^[ ]{2}ANDROID_PLATFORM_TOOLS_ARCHIVE_URL: "https:\/\/mirrors\.cloud\.tencent\.com\/AndroidSDK\/platform-tools_r37\.0\.1-linux\.zip"$/mu,
  );
  assert.match(
    variables,
    /^[ ]{2}ANDROID_NDK_ARCHIVE_URL: "https:\/\/mirrors\.cloud\.tencent\.com\/AndroidSDK\/android-ndk-r29-linux\.zip"$/mu,
  );
  assert.doesNotMatch(variables, /SHA256/u);
  assert.match(source, /source scripts\/ci\/configure-ubuntu-apt-mirror\.sh/u);
  assert.match(source, /aria2 build-essential ca-certificates curl/u);
  const linuxBootstrap = await readRepositoryFile(
    "scripts/ci/bootstrap-gitlab-linux.sh",
  );
  assert.match(linuxBootstrap, /download_with_parallel_ranges/u);
  assert.match(linuxBootstrap, /--max-connection-per-server=16/u);
  assert.match(
    linuxBootstrap,
    /if \[\[ ! -s "\$android_license" \]\]; then/u,
  );
  assert.match(
    linuxBootstrap,
    /mirrors\.cloud\.tencent\.com\/AndroidSDK\/commandlinetools-linux-/u,
  );
  assert.match(linuxBootstrap, /replace-with = "rsproxy"/u);
  const windowsBootstrap = await readRepositoryFile(
    "scripts/ci/bootstrap-gitlab-windows.ps1",
  );
  assert.match(windowsBootstrap, /replace-with = "rsproxy"/u);
  assert.match(windowsBootstrap, /DENO_DIST_BASE_URL/u);
  assert.match(linuxBootstrap, /DENO_DIST_BASE_URL/u);
  assert.match(windowsBootstrap, /VersionLine -match '\^deno\\s\+\(\\S\+\)'/u);
  assert.match(linuxBootstrap, /awk 'NR == 1 \{ print \$2; exit \}'/u);
  assert.match(windowsBootstrap, /Get-Command cl\.exe/u);
  assert.match(windowsBootstrap, /vcvars64\.bat/u);
  assert.match(
    windowsBootstrap,
    /Microsoft\.VisualStudio\.Workload\.VCTools/u,
  );
  assert.match(windowsBootstrap, /--includeRecommended/u);
  assert.match(windowsBootstrap, /Start-Process/u);
  assert.match(windowsBootstrap, /ExitCode -notin @\(0, 3010\)/u);
  assert.match(windowsBootstrap, /usr\/bin\/perl\.exe/u);
  assert.match(
    windowsBootstrap,
    /exit\(\$\^O eq "MSWin32" \? 0 : 1\)/u,
  );
  assert.match(windowsBootstrap, /WINDOWS_PERL_PACKAGE_URL/u);
  assert.match(windowsBootstrap, /WINDOWS_PERL_GCC_LIBS_PACKAGE_URL/u);
  assert.match(windowsBootstrap, /WINDOWS_PERL_PTHREAD_PACKAGE_URL/u);
  assert.match(windowsBootstrap, /WINDOWS_PERL_TZDATA_PACKAGE_URL/u);
  assert.match(windowsBootstrap, /Tsinghua MSYS2 mirror/u);
  assert.match(windowsBootstrap, /eval --no-lock/u);
  assert.match(windowsBootstrap, /npm:fzstd@0\.1\.1/u);
  assert.match(windowsBootstrap, /\$env:PERL5LIB/u);
  assert.match(
    windowsBootstrap,
    /A native Windows Perl runtime with OpenSSL's required core modules is required to build vendored OpenSSL on Windows/u,
  );
  assert.match(
    windowsBootstrap,
    /Adding Perl to PATH replaced the MSVC linker/u,
  );
  assert.doesNotMatch(
    linuxBootstrap,
    /"ndk;\$ANDROID_NDK_VERSION"/u,
  );
  assert.doesNotMatch(
    linuxBootstrap,
    /"build-tools;\$ANDROID_BUILD_TOOLS_VERSION"/u,
  );
  const androidSettings = await readRepositoryFile(
    "android-companion/settings.gradle.kts",
  );
  assert.match(androidSettings, /maven\.aliyun\.com\/repository\/google/u);
  assert.match(androidSettings, /maven\.aliyun\.com\/repository\/central/u);
  assert.match(source, /deno ci/u);
  assert.doesNotMatch(source, /actions\/setup-node|pnpm\/action-setup/u);
  assert.doesNotMatch(source, /(?:^|\s)(?:node|npm|pnpm)\s/u);
});

test("Android and Windows build-only mode packages the Companion without running other gates", async () => {
  const source = await readWorkflow();
  const androidEnvironmentJob = topLevelBlock(source, "android_environment");
  const tauriConfig = JSON.parse(
    await readRepositoryFile("src-tauri/tauri.conf.json"),
  );
  const portableScript = await readRepositoryFile(
    "scripts/package-portable.ps1",
  );
  const androidJob = topLevelBlock(source, "android_companion");
  const coverageJob = topLevelBlock(source, "coverage_gates");
  const verifyJob = topLevelBlock(source, "verify_windows");
  const packageJob = topLevelBlock(source, "package_windows_unsigned");
  const buildOnlyJob = topLevelBlock(source, "windows_build_only");

  assert.match(
    androidEnvironmentJob,
    /PIPELINE_MODE == "android-windows-build" && \(\$CI_PIPELINE_SOURCE == "api" \|\| \$CI_PIPELINE_SOURCE == "web"\)/u,
  );
  assert.match(
    androidEnvironmentJob,
    /source scripts\/ci\/bootstrap-gitlab-linux\.sh android/u,
  );
  assert.doesNotMatch(
    androidEnvironmentJob,
    /(?:^|\n)[ ]*-[ ]+(?:deno task|cargo test|gradle|lint|clippy|audit)(?:[ ]|$)/u,
  );
  assert.match(
    androidJob,
    /job: android_environment\n[ ]{6}optional: true/u,
  );

  assert.match(
    androidJob,
    /PIPELINE_MODE" = "android-windows-build"[^]*?gradle --no-daemon :app:assembleRelease/u,
  );
  assert.match(
    androidJob,
    /android-companion-artifact\/intercept-proxy-android-companion\.apk/u,
  );

  for (const skippedJob of [coverageJob, verifyJob, packageJob]) {
    assert.match(
      skippedJob,
      /PIPELINE_MODE == "android-windows-build"[^]*?when: never/u,
    );
  }

  assert.match(buildOnlyJob, /^[ ]{4}- "slave4"$/mu);
  assert.match(buildOnlyJob, /^[ ]{2}timeout: 2h$/mu);
  assert.match(
    buildOnlyJob,
    /^[ ]{4}GIT_CLEAN_FLAGS: "-ffdx -e \.ci-cache\/ -e src-tauri\/target\/"$/mu,
  );
  assert.match(buildOnlyJob, /^[ ]{4}when: always$/mu);
  assert.match(
    buildOnlyJob,
    /PIPELINE_MODE == "android-windows-build" && \(\$CI_PIPELINE_SOURCE == "api" \|\| \$CI_PIPELINE_SOURCE == "web"\)/u,
  );
  assert.match(buildOnlyJob, /job: android_companion[^]*?artifacts: true/u);
  assert.match(buildOnlyJob, /stage-android-companion-windows\.ps1/u);
  assert.match(buildOnlyJob, /deno task tauri build -vv --bundles msi,nsis/u);
  assert.match(buildOnlyJob, /WixTools314/u);
  assert.match(buildOnlyJob, /candle\.exe/u);
  assert.match(buildOnlyJob, /Candle diagnostics were printed above/u);
  assert.match(buildOnlyJob, /scripts[\\/]package-portable\.ps1 -SkipBuild/u);
  assert.match(
    buildOnlyJob,
    /src-tauri\/target\/release\/bundle\/msi\/\*\.msi/u,
  );
  assert.match(
    buildOnlyJob,
    /src-tauri\/target\/release\/bundle\/nsis\/\*\.exe/u,
  );
  assert.match(buildOnlyJob, /dist\/\*\.zip/u);
  assert.deepEqual(tauriConfig.bundle.resources, [
    "resources/android-companion.apk",
  ]);
  assert.match(
    portableScript,
    /Copy-Item \$CompanionApk \$PackagedCompanionApk/u,
  );
  assert.match(
    portableScript,
    /resources\/android-companion\.apk is installed on a selected Android device/u,
  );
  assert.doesNotMatch(
    buildOnlyJob,
    /deno (?:audit|task (?:test|lint|typecheck|scan|check))|cargo (?:test|clippy|fmt|check)|verify-windows-openssl-runtime|Get-AuthenticodeSignature/u,
  );
});

test("Windows packaging is gated, explicit unsigned and preserves all deliverables", async () => {
  const source = await readWorkflow();
  const packageJob = topLevelBlock(source, "package_windows_unsigned");

  assert.match(
    packageJob,
    /^package_windows_unsigned:\n[ ]{2}stage: package/mu,
  );
  assert.match(packageJob, /job: verify_windows/u);
  assert.match(packageJob, /job: coverage_gates/u);
  assert.match(
    packageJob,
    /CI_PIPELINE_SOURCE == "merge_request_event"[^]*?when: never/u,
  );
  assert.match(packageJob, /deno task tauri build --bundles msi,nsis/u);
  assert.match(packageJob, /Get-AuthenticodeSignature/u);
  assert.match(packageJob, /\$Signature\.Status -ne "NotSigned"/u);
  assert.match(packageJob, /scripts[\\/]verify-windows-openssl-runtime\.ps1/u);
  assert.match(packageJob, /scripts[\\/]package-portable\.ps1 -SkipBuild/u);
  assert.match(packageJob, /src-tauri\/target\/release\/bundle\/msi\/\*\.msi/u);
  assert.match(
    packageJob,
    /src-tauri\/target\/release\/bundle\/nsis\/\*\.exe/u,
  );
  assert.match(packageJob, /dist\/\*\.zip/u);
  assert.doesNotMatch(
    source,
    /WINDOWS_CERTIFICATE|WINDOWS_CERTIFICATE_PASSWORD|WINDOWS_TIMESTAMP_URL|Import-PfxCertificate/u,
  );
});
