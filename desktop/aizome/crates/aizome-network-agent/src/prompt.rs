use crate::{
    AgentError, Result,
    model::Prompt,
    process::{self, Cancel},
};
use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use std::{collections::HashMap, process::Command};

pub trait Prompter: Send + Sync {
    fn ask(&self, prompt: Prompt, cancel: &Cancel) -> Result<HashMap<String, String>>;
}
pub struct Native;
impl Prompter for Native {
    fn ask(&self, prompt: Prompt, cancel: &Cancel) -> Result<HashMap<String, String>> {
        let exe =
            std::env::current_exe().map_err(|_| AgentError::Failed("Cannot locate authentication dialog".into()))?;
        let bytes = process::run(
            Command::new(exe).arg("--prompt"),
            serde_json::to_vec(&prompt).map_err(|_| AgentError::Failed("Invalid prompt".into()))?,
            cancel,
            // A cancelled dialog answers `null`. This one couldn't open or
            // stopped, so NetworkManager may still ask other agents.
            AgentError::Failed("The authentication dialog failed".into()),
        )?;
        serde_json::from_slice::<Option<HashMap<String, String>>>(&bytes)
            .map_err(|_| AgentError::Failed("Invalid authentication response".into()))?
            .ok_or_else(|| AgentError::UserCanceled("Authentication cancelled".into()))
    }
}
#[derive(Clone)]
pub enum Msg {
    Edit(usize, String),
    Submit,
    Cancel,
}
pub struct Dialog {
    pub prompt: Prompt,
    pub answer: Option<HashMap<String, String>>,
}
impl Program for Dialog {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.NetworkAuthentication", "Network Authentication", "Network Authentication")
            .icon(Icon::Wifi)
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(
            WindowSpec::new(&self.prompt.title)
                .size(440.0, (180.0 + 68.0 * self.prompt.fields.len() as f64).min(640.0)),
        );
        cx.focus("network-secret-0");
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Edit(i, value) => {
                if let Some(f) = self.prompt.fields.get_mut(i) {
                    f.value = value;
                }
            }
            Msg::Submit if self.prompt.fields.iter().all(|f| !f.required || !f.value.is_empty()) => {
                self.answer = Some(self.prompt.fields.iter().map(|f| (f.key.clone(), f.value.clone())).collect());
                cx.quit();
            }
            Msg::Cancel => cx.quit(),
            _ => {}
        }
    }
    fn view(&self, _window: WindowKey) -> Element<Msg> {
        let mut content = column![
            text(self.prompt.title.as_str()).semibold().size(17.0),
            text(self.prompt.message.as_str()).wrap().role(Role::Text2)
        ]
        .spacing(12.0)
        .padding(20.0);
        for (i, f) in self.prompt.fields.iter().enumerate() {
            let field = text_field(&f.value)
                .id(format!("network-secret-{i}"))
                .on_change(move |value| Msg::Edit(i, value))
                .on_submit(Msg::Submit);
            let field = if f.secret { field.password() } else { field };
            content = content.push(column![text(f.label.as_str()), field].spacing(5.0));
        }
        scroll(
            content.push(
                row![
                    spacer(),
                    button("Cancel").on_press(Msg::Cancel),
                    button("Connect")
                        .primary()
                        .on_press(Msg::Submit)
                        .enabled(self.prompt.fields.iter().all(|f| !f.required || !f.value.is_empty()))
                ]
                .spacing(8.0),
            ),
        )
        .into()
    }
}
