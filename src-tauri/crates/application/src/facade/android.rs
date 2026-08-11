use super::Application;
use crate::{
    AndroidAdbViewModel, AndroidDeviceViewModel, AndroidNetworkProfile, AndroidNetworkState,
    AndroidNetworkStatusViewModel, AndroidPackageViewModel, AndroidTargetApplication, AppError,
    AppResult,
};
use std::collections::BTreeSet;

mod activation;
mod control;
mod packages;
mod profiles;
mod runtime;

#[cfg(test)]
use packages::filter_packages;
use packages::{apply_package_toggle, validate_package_name, validate_profile_id};

#[cfg(test)]
#[path = "android_tests.rs"]
mod tests;
