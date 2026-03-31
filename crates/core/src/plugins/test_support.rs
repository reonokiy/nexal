use crate::config::CONFIG_TOML_FILE;
use crate::config::ConfigBuilder;
use std::fs;
use std::path::Path;

pub(crate) fn write_file(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("file should have a parent")).unwrap();
    fs::write(path, contents).unwrap();
}

pub(crate) fn write_plugins_feature_config(nexal_home: &Path) {
    write_file(
        &nexal_home.join(CONFIG_TOML_FILE),
        r#"[features]
plugins = true
"#,
    );
}

pub(crate) async fn load_plugins_config(nexal_home: &Path) -> crate::config::Config {
    ConfigBuilder::default()
        .nexal_home(nexal_home.to_path_buf())
        .fallback_cwd(Some(nexal_home.to_path_buf()))
        .build()
        .await
        .expect("config should load")
}
