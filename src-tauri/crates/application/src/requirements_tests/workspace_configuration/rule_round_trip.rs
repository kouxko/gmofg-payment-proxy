use super::protocol_package_portability::{
    description, package, portable_package, scripted_workspace,
};
use super::*;

#[tokio::test]
async fn relay_and_local_responder_round_trip_all_rule_identity_revision_and_order_fields() {
    for local_responder in [false, true] {
        let package = package("multi-rule", "4.2.0");
        let mut source = scripted_workspace(package.clone(), local_responder);
        let listener_id = source.listeners[0].id;
        let direction = if local_responder {
            SocketDirection::Downstream
        } else {
            SocketDirection::Upstream
        };

        let mut first = source.socket_rules.remove(0);
        let mut draft = first.to_draft();
        draft.priority = 20;
        let revision = first.update(first.revision(), draft).unwrap();
        first.toggle(revision, false).unwrap();
        let mut second = SocketDocumentRuleDefinition::new(
            SocketDocumentRuleId::new(),
            true,
            -20,
            70,
            listener_id,
            package.clone(),
            7,
            direction,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        )
        .unwrap();
        second.toggle(second.revision(), false).unwrap();
        source.socket_rules = vec![second, first];
        source.socket_rule_created_order_high_water = 70;
        source.validate().unwrap();
        assert_eq!(
            source
                .socket_rules
                .iter()
                .map(|rule| (rule.priority(), rule.created_order(), rule.revision().get()))
                .collect::<Vec<_>>(),
            vec![(-20, 70, 2), (20, 41, 3)]
        );

        let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
        let source = workspaces.import_workspace(source).await.unwrap();
        workspaces.select(source.id).await.unwrap();
        let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
        let (application, portability) = application_with_workspace_configuration_and_packages(
            Arc::new(FakePorts::default()),
            workspaces.clone(),
            documents.clone(),
            Arc::new(UnavailableApplicationConfigurationStore),
        );
        portability.register(
            portable_package(package.clone(), false),
            description(package.clone()),
        );

        application.workspace_export(source.id).await.unwrap();
        let (_, bytes) = documents.take_last_export().unwrap();
        documents.set_next_import(bytes);
        application.workspace_import().await.unwrap();
        let imported = workspaces
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|summary| summary.id != source.id)
            .map(|summary| workspaces.get(summary.id))
            .unwrap()
            .await
            .unwrap();

        let mut expected = source.socket_rules.clone();
        for rule in &mut expected {
            rule.rebind_listener_for_workspace_remap(imported.listeners[0].id)
                .unwrap();
        }
        assert_eq!(imported.socket_rules, expected);
        let SocketPayloadProcessing::Scripted(processing) =
            &imported.listeners[0].socket().unwrap().processing
        else {
            panic!("Scripted listener must survive round-trip")
        };
        assert_eq!(processing.package, package);
        assert!(imported.socket_rules.iter().all(|rule| {
            rule.listener_id() == imported.listeners[0].id && rule.package() == &processing.package
        }));
    }
}
