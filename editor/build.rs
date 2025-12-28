use std::env;

fn main() {
    let assets_path = format!(
        "{}/assets",
        env::var_os("CARGO_MANIFEST_DIR").unwrap().to_str().unwrap()
    );
    let assets_out_path = format!(
        "{}/target/{}/assets",
        env::var_os("CARGO_MANIFEST_DIR").unwrap().to_str().unwrap(),
        env::var_os("PROFILE").unwrap().to_str().unwrap(),
    );

    if std::fs::symlink_metadata(assets_out_path.clone()).is_err() {
        if cfg!(unix) {
            std::os::unix::fs::symlink(assets_path, assets_out_path)
                .expect("Our previous check if the symlink already exists failed somehow.");
        } else if cfg!(windows) {
            println!("warning=TODO: Copy asset dir on windows and println changes on asset dir. Print stamp files to the out dir (if those files persist between builds), to prevent copying every single asset.");
        } else {
            println!("warning=Can't symlink assets dir.");
        }
    }
}
