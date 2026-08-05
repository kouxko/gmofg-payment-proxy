use super::safe_file_stem;

#[test]
fn export_file_name_cannot_escape_selected_directory() {
    assert_eq!(safe_file_stem("../Lab Workspace"), ".._Lab_Workspace");
    assert_eq!(safe_file_stem("  "), "workspace");
}
