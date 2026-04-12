use rogue_engine::asset::repr::project::ProjectAsset;

pub struct RuntimeProjectLoader;

impl RuntimeProjectLoader {
    pub fn load_project() -> ProjectAsset {
        let project_dir = std::env::current_dir()
            .expect("Couldn't get current directory.")
            .join("project_data");
        ProjectAsset::from_existing_raw(&project_dir, crate::init_ecs_world())
            .map_err(|err| {
                panic!(
                    "Error when trying to deserialize last project. Error: {:?}",
                    err
                );
            })
            .unwrap()
    }
}
