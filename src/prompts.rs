use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use uuid::Uuid;

use crate::i18n::{AppLanguage, Key, tr};
use crate::model::ChatParams;
use crate::paths::{PROMPTS_FILE, data_file, write_atomic};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PromptPreset {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub system_prompt: String,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub params: ChatParams,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub id: String,
    pub name: String,
    pub body: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PromptLibrary {
    #[serde(default)]
    pub presets: Vec<PromptPreset>,
    #[serde(default)]
    pub templates: Vec<PromptTemplate>,
}

impl PromptLibrary {
    /// 读提示词库。文件不存在时按 `lang` 生成一份内置的，并落盘。
    ///
    /// 内置内容只在**首次运行**时生成：之后就是用户自己的数据了，换界面语言不该改动它。
    /// 所以这里传语言是安全的——它只影响"种子内容"，不会让已有数据变脸。
    pub fn load(lang: AppLanguage) -> Self {
        let path = data_file(PROMPTS_FILE);
        if !path.exists() {
            let library = Self::defaults(lang);
            // 写不进去也无所谓：默认提示词库是代码里生成的，下次启动会再建一遍
            let _ = library.save();
            return library;
        }
        fs::read_to_string(&path)
            .and_then(|text| {
                serde_json::from_str(&text).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
            })
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        write_atomic(&data_file(PROMPTS_FILE), &json)
    }

    /// 内置的预设与模板。文案复用界面文案里已有的 `Preset*` key——
    /// 空状态那四个快捷入口用的是同一批字符串，没必要再抄一份。
    pub fn defaults(lang: AppLanguage) -> Self {
        Self {
            presets: vec![PromptPreset {
                id: Uuid::new_v4().to_string(),
                name: tr(lang, Key::DefaultPresetName).into(),
                icon: "✨".into(),
                system_prompt: String::new(),
                provider_id: String::new(),
                model: String::new(),
                params: ChatParams::default(),
            }],
            templates: vec![
                template(tr(lang, Key::PresetTranslate), tr(lang, Key::PresetTranslatePrompt)),
                template(tr(lang, Key::PresetPolish), tr(lang, Key::PresetPolishPrompt)),
                template(tr(lang, Key::PresetSummary), tr(lang, Key::PresetSummaryPrompt)),
                template(
                    tr(lang, Key::PresetExplainCode),
                    tr(lang, Key::PresetExplainCodeTemplate),
                ),
            ],
        }
    }
}

fn template(name: &str, body: &str) -> PromptTemplate {
    PromptTemplate {
        id: Uuid::new_v4().to_string(),
        name: name.to_string(),
        body: body.to_string(),
    }
}

pub fn expand_variables(body: &str, clipboard: &str, selection: &str) -> String {
    let date = Local::now().format("%Y-%m-%d").to_string();
    body.replace("{{date}}", &date)
        .replace("{{clipboard}}", clipboard)
        .replace("{{selection}}", selection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_known_variables_and_leaves_unknown_text() {
        let expanded = expand_variables(
            "今天是 {{date}}，剪贴板：{{clipboard}}，选区：{{selection}}",
            "copied",
            "selected",
        );
        assert!(expanded.contains("copied"));
        assert!(expanded.contains("selected"));
        assert!(expanded.contains(&Local::now().format("%Y-%m-%d").to_string()));
        assert!(!expanded.contains("{{clipboard}}"));
    }
}
