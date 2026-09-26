use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use uuid::Uuid;

use crate::model::ChatParams;
use crate::paths::{data_file, write_atomic};

const FILE_NAME: &str = "prompts.json";

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
    pub fn load() -> Self {
        let path = data_file(FILE_NAME);
        if !path.exists() {
            let library = Self::defaults();
            let _ = library.save();
            return library;
        }
        match fs::read_to_string(&path).and_then(|text| {
            serde_json::from_str(&text).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        }) {
            Ok(library) => library,
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        write_atomic(&data_file(FILE_NAME), &json)
    }

    pub fn defaults() -> Self {
        Self {
            presets: vec![PromptPreset {
                id: Uuid::new_v4().to_string(),
                name: "通用助手".into(),
                icon: "✨".into(),
                system_prompt: String::new(),
                provider_id: String::new(),
                model: String::new(),
                params: ChatParams::default(),
            }],
            templates: vec![
                template("翻译", "请把下面的内容翻译成英文（如果原文是英文则翻译成中文），保留原有格式：\n"),
                template("润色文字", "请帮我润色下面这段文字，使表达更通顺专业，并说明主要改动：\n"),
                template("总结要点", "请用要点的形式总结下面的内容：\n"),
                template("解释代码", "请逐段解释下面这段代码：\n{{selection}}"),
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
        let expanded = expand_variables("今天是 {{date}}，剪贴板：{{clipboard}}，选区：{{selection}}", "copied", "selected");
        assert!(expanded.contains("copied"));
        assert!(expanded.contains("selected"));
        assert!(expanded.contains(&Local::now().format("%Y-%m-%d").to_string()));
        assert!(!expanded.contains("{{clipboard}}"));
    }
}
