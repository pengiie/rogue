use serde::ser::SerializeStruct;

use crate::ui::{
    entity_hierarchy::EntityHierarchyUI, entity_properties::EntityPropertiesPane,
    materials_pane::MaterialsPane, world_pane::WorldPane, EditorUIContext,
};

pub struct EditorUIPaneData {
    pub id: String,
    pub pane: Box<dyn EditorUIPaneMethods>,
}

impl EditorUIPaneData {
    pub fn name(&self) -> &str {
        self.pane.name()
    }

    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        self.pane.show(ui, ctx);
    }

    pub fn open_pane(&mut self, pane_id: &str) -> bool {
        self.pane.open_pane(pane_id)
    }

    /// Returns Some with the given pane if it couldn't be spawned anywhere.
    pub fn spawn_pane(&mut self, pane: EditorUIPaneData) -> Option<EditorUIPaneData> {
        self.pane.spawn_pane(pane)
    }
}

impl<'de> serde::Deserialize<'de> for EditorUIPaneData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_struct(
            "EditorUIPaneData",
            &["id", "pane"],
            EditorUIPaneDataVisitor,
        )
    }
}

struct EditorUIPaneDataVisitor;

struct EditorUIPaneDataDeserializeSeed {
    id: String,
}

impl<'de> serde::de::DeserializeSeed<'de> for EditorUIPaneDataDeserializeSeed {
    type Value = Box<dyn EditorUIPaneMethods>;

    fn deserialize<D>(self, de: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        fn deserialize_pane<
            'de,
            T: EditorUIPane + serde::Deserialize<'de>,
            D: serde::Deserializer<'de>,
        >(
            de: D,
        ) -> Result<Box<dyn EditorUIPaneMethods>, D::Error> {
            <T as serde::Deserialize>::deserialize(de)
                .map(|pane| Box::new(pane) as Box<dyn EditorUIPaneMethods>)
        }

        match self.id.as_str() {
            EditorUISplitPane::ID => deserialize_pane::<EditorUISplitPane, D>(de),
            EditorUITabPane::ID => deserialize_pane::<EditorUITabPane, D>(de),
            EditorUIContentPane::ID => deserialize_pane::<EditorUIContentPane, D>(de),
            EntityHierarchyUI::ID => deserialize_pane::<EntityHierarchyUI, D>(de),
            MaterialsPane::ID => deserialize_pane::<MaterialsPane, D>(de),
            EntityPropertiesPane::ID => deserialize_pane::<EntityPropertiesPane, D>(de),
            WorldPane::ID => deserialize_pane::<WorldPane, D>(de),
            _ => panic!("Unknown pane id: {}", self.id),
        }
    }
}

impl<'de> serde::de::Visitor<'de> for EditorUIPaneDataVisitor {
    type Value = EditorUIPaneData;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("struct EditorUIPaneData")
    }

    fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
    where
        V: serde::de::MapAccess<'de>,
    {
        let mut id = None;
        let mut pane = None;
        while let Some(key) = map.next_key()? {
            match key {
                "id" => {
                    if id.is_some() {
                        return Err(serde::de::Error::duplicate_field("id"));
                    }

                    id = Some(map.next_value::<String>()?);
                }
                "pane" => {
                    if pane.is_some() {
                        return Err(serde::de::Error::duplicate_field("pane"));
                    }

                    let id = id
                        .as_ref()
                        .expect("id should be deserialized before pane")
                        .clone();
                    let seed = EditorUIPaneDataDeserializeSeed { id };
                    pane = Some(map.next_value_seed(seed)?);
                }
                _ => {
                    return Err(serde::de::Error::unknown_field(key, &["id", "pane"]));
                }
            }
        }

        let id = id.ok_or_else(|| serde::de::Error::missing_field("id"))?;
        let pane = pane.ok_or_else(|| serde::de::Error::missing_field("pane"))?;

        Ok(EditorUIPaneData { id, pane })
    }
}

impl serde::Serialize for EditorUIPaneData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("EditorUIPaneData", 2)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("pane", &self.pane)?;
        state.end()
    }
}

pub trait EditorUIPane: erased_serde::Serialize + 'static {
    const ID: &'static str = "null";
    const NAME: &'static str = "Pane";

    fn name(&self) -> &str {
        Self::NAME
    }

    fn into_pane(self) -> EditorUIPaneData
    where
        Self: Sized + 'static,
    {
        EditorUIPaneData {
            id: Self::ID.to_owned(),
            pane: Box::new(self),
        }
    }

    fn into_content_pane(self) -> EditorUIPaneData
    where
        Self: Sized + 'static,
    {
        EditorUIContentPane::new(self).into_pane()
    }

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>);

    /// Returns true if the pane was opened or is already opened, false if it doesn't exist.
    fn open_pane(&mut self, pane_id: &str) -> bool {
        pane_id == Self::ID
    }

    fn spawn_pane(&mut self, pane: EditorUIPaneData) -> Option<EditorUIPaneData> {
        Some(pane)
    }
}

pub trait EditorUIPaneMethods: erased_serde::Serialize {
    fn name(&self) -> &str;
    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>);
    fn open_pane(&mut self, pane_id: &str) -> bool;
    fn spawn_pane(&mut self, pane: EditorUIPaneData) -> Option<EditorUIPaneData>;
}

erased_serde::serialize_trait_object!(EditorUIPaneMethods);

impl<T: EditorUIPane + erased_serde::Serialize> EditorUIPaneMethods for T {
    fn name(&self) -> &str {
        <T as EditorUIPane>::name(self)
    }

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        <T as EditorUIPane>::show(self, ui, ctx);
    }

    fn open_pane(&mut self, pane_id: &str) -> bool {
        <T as EditorUIPane>::open_pane(self, pane_id)
    }

    fn spawn_pane(&mut self, pane: EditorUIPaneData) -> Option<EditorUIPaneData> {
        <T as EditorUIPane>::spawn_pane(self, pane)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EditorUISplitPane {
    pub sub_panes: Vec<EditorUIPaneData>,
    pub split_direction: SplitDirection,
}

impl EditorUIPane for EditorUISplitPane {
    const ID: &'static str = "split_pane";

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        let layout = match self.split_direction {
            SplitDirection::Horizontal => egui::Layout::left_to_right(egui::Align::Min),
            SplitDirection::Vertical => egui::Layout::top_down(egui::Align::Min),
        };
        ui.with_layout(layout, |ui| {
            for (i, pane) in self.sub_panes.iter_mut().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                pane.show(ui, ctx);
            }
        });
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EditorUITabPane {
    pub tabs: Vec<EditorUIPaneData>,
    pub selected_tab: usize,
}

impl EditorUITabPane {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            selected_tab: 0,
        }
    }

    pub fn with_child(pane: EditorUIPaneData) -> Self {
        Self {
            tabs: vec![pane],
            selected_tab: 0,
        }
    }
}

impl EditorUIPane for EditorUITabPane {
    const ID: &'static str = "tab_pane";

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        if self.tabs.is_empty() {
            ui.label("Empty tab layout.");
            return;
        }

        let show_tabs = self.tabs.len() > 1;
        if show_tabs {
            ui.horizontal(|ui| {
                ui.style_mut().spacing.item_spacing.x = 0.0;
                for (i, pane) in self.tabs.iter().enumerate() {
                    if ui
                        .add_enabled(self.selected_tab != i, egui::Button::new(pane.name()))
                        .clicked()
                    {
                        self.selected_tab = i;
                    }
                }
            });
        }

        self.tabs[self.selected_tab].show(ui, ctx);
    }

    fn spawn_pane(&mut self, pane: EditorUIPaneData) -> Option<EditorUIPaneData> {
        self.selected_tab = self.tabs.len();
        self.tabs.push(pane);
        None
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EditorUIContentPane {
    pub content: EditorUIPaneData,
}

impl EditorUIContentPane {
    pub fn new(content: impl EditorUIPane) -> Self {
        Self {
            content: content.into_pane(),
        }
    }
}

impl EditorUIPane for EditorUIContentPane {
    const ID: &'static str = "scroll_pane";

    fn name(&self) -> &str {
        self.content.name()
    }

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        self.content.show(ui, ctx);
                    });
            });
    }

    fn open_pane(&mut self, pane_id: &str) -> bool {
        self.content.open_pane(pane_id)
    }
}
