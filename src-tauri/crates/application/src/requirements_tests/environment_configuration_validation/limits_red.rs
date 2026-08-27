use super::*;

fn candidate_json_with(edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut candidate: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    edit(&mut candidate);
    serde_json::to_vec(&candidate).unwrap()
}

async fn assert_domain_code(candidate: &[u8], expected: EnvironmentStatusCode) {
    let report = validator(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
        .validate(candidate, CancellationToken::new())
        .await;

    assert_eq!(
        report.layers()[1].layer(),
        EnvironmentValidationLayer::Domain
    );
    assert_eq!(report.layers()[1].code(), Some(expected));
    assert_eq!(report.status_code(), Some(expected));
}

#[tokio::test]
async fn rejects_more_than_sixty_four_android_target_applications() {
    let candidate = candidate_json_with(|candidate| {
        let applications =
            candidate["workspace"]["android_network_profiles"][0]["target_applications"]
                .as_array_mut()
                .unwrap();
        let template = applications[0].clone();
        while applications.len() <= 64 {
            let mut application = template.clone();
            application["package_name"] =
                serde_json::json!(format!("test.package.{}", applications.len()));
            application["uid"] = serde_json::json!(10_000 + applications.len());
            applications.push(application);
        }
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_android_profiles_without_a_target_application() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["android_network_profiles"][0]["target_applications"] =
            serde_json::json!([]);
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_android_profile_ids_longer_than_one_hundred_twenty_eight_characters() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["android_network_profiles"][0]["id"] =
            serde_json::json!("a".repeat(129));
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_more_than_one_hundred_twenty_eight_android_destination_targets() {
    let candidate = candidate_json_with(|candidate| {
        let targets = candidate["workspace"]["android_network_profiles"][0]["destination_targets"]
            .as_array_mut()
            .unwrap();
        let template = targets[0].clone();
        while targets.len() <= 128 {
            let mut target = template.clone();
            target["cidr"] = serde_json::json!(format!("10.0.{}.0/24", targets.len()));
            targets.push(target);
        }
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_more_than_one_hundred_twenty_eight_android_proxy_routes() {
    let candidate = candidate_json_with(|candidate| {
        let routes = candidate["workspace"]["android_network_profiles"][0]["proxy_routes"]
            .as_array_mut()
            .unwrap();
        let template = routes[0].clone();
        while routes.len() <= 128 {
            let mut route = template.clone();
            route["destination"] = serde_json::json!(format!("10.1.{}.1", routes.len()));
            routes.push(route);
        }
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_android_profile_names_longer_than_eighty_characters() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["android_network_profiles"][0]["name"] =
            serde_json::json!("n".repeat(81));
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_android_weak_network_json_larger_than_two_hundred_fifty_six_kibibytes() {
    let candidate = candidate_json_with(|candidate| {
        let windows = candidate["workspace"]["android_network_profiles"][0]["weak_network"]
            ["blackout_windows"]
            .as_array_mut()
            .unwrap();
        let template = windows[0].clone();
        while serde_json::to_vec(&windows).unwrap().len() <= 262_144 {
            windows.push(template.clone());
        }
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_certificate_content_larger_than_two_hundred_fifty_six_decoded_kibibytes() {
    let candidate = candidate_json_with(|candidate| {
        candidate["materials"]["certificates"][0]["encoding"] = serde_json::json!("base64_der");
        candidate["materials"]["certificates"][0]["content"] =
            serde_json::json!("A".repeat(349_528));
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_certificate_aliases_longer_than_sixty_four_ascii_bytes() {
    let candidate = candidate_json_with(|candidate| {
        let alias = "a".repeat(65);
        candidate["workspace"]["listeners"][0]["data_plane"]["settings"]["downstream_tls"]["server_identity_alias"] =
            serde_json::json!(alias.clone());
        candidate["materials"]["certificates"][0]["alias"] = serde_json::json!(alias);
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_certificate_passwords_larger_than_four_kibibytes() {
    let candidate = candidate_json_with(|candidate| {
        candidate["materials"]["certificates"][4]["password"] =
            serde_json::json!("p".repeat(4_097));
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_secret_passwords_larger_than_four_kibibytes() {
    let candidate = candidate_json_with(|candidate| {
        candidate["materials"]["secrets"][0]["password"] = serde_json::json!("p".repeat(4_097));
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_secret_aliases_longer_than_sixty_four_ascii_bytes() {
    let candidate = candidate_json_with(|candidate| {
        let alias = "a".repeat(65);
        candidate["workspace"]["listeners"][0]["data_plane"]["settings"]["authentication"]["credential_alias"] =
            serde_json::json!(alias.clone());
        candidate["materials"]["secrets"][0]["alias"] = serde_json::json!(alias);
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

async fn assert_socket_listener_invalid(edit: impl FnOnce(&mut serde_json::Value)) {
    let candidate = candidate_json_with(edit);
    assert_domain_code(&candidate, EnvironmentStatusCode::ListenerDomainInvalid).await;
}

#[tokio::test]
async fn rejects_socket_maximum_connections_above_five_thousand() {
    assert_socket_listener_invalid(|candidate| {
        candidate["workspace"]["listeners"][1]["data_plane"]["settings"]["maximum_connections"] =
            serde_json::json!(5_001);
    })
    .await;
}

#[tokio::test]
async fn rejects_zero_socket_maximum_connections() {
    assert_socket_listener_invalid(|candidate| {
        candidate["workspace"]["listeners"][1]["data_plane"]["settings"]["maximum_connections"] =
            serde_json::json!(0);
    })
    .await;
}

#[tokio::test]
async fn rejects_zero_socket_read_chunk_bytes() {
    assert_socket_listener_invalid(|candidate| {
        candidate["workspace"]["listeners"][1]["data_plane"]["settings"]["runtime_limits"]
            ["read_chunk_bytes"] = serde_json::json!(0);
    })
    .await;
}

#[tokio::test]
async fn rejects_zero_socket_diagnostic_event_capacity() {
    assert_socket_listener_invalid(|candidate| {
        candidate["workspace"]["listeners"][1]["data_plane"]["settings"]["runtime_limits"]
            ["diagnostic_event_capacity"] = serde_json::json!(0);
    })
    .await;
}

#[tokio::test]
async fn rejects_zero_socket_diagnostic_memory_bytes() {
    assert_socket_listener_invalid(|candidate| {
        candidate["workspace"]["listeners"][1]["data_plane"]["settings"]["runtime_limits"]
            ["diagnostic_memory_bytes"] = serde_json::json!(0);
    })
    .await;
}
