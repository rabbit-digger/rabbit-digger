use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Result};
use rd_interface::config::Value;
use serde_derive::{Deserialize, Serialize};

pub type ConfigNet = HashMap<String, Net>;
pub type ConfigServer = HashMap<String, Server>;
pub type ConfigComposite = HashMap<String, CompositeName>;

#[derive(Debug)]
pub enum AllNet {
    Net(Net),
    Composite(CompositeName),
    Local,
    Noop,
}

impl AllNet {
    pub fn get_dependency(&self) -> Vec<String> {
        match self {
            AllNet::Net(Net { chain, .. }) => match chain {
                Chain::One(s) => vec![s.to_string()],
                Chain::Many(v) => v.iter().map(Clone::clone).collect(),
            },
            AllNet::Composite(CompositeName { composite, .. }) => match &composite.0 {
                Composite::Rule(CompositeRule { rule }) => {
                    rule.iter().map(|i| i.target.clone()).collect()
                }
            },
            _ => Vec::new()
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "plugins")]
    pub plugin_path: PathBuf,
    #[serde(default)]
    pub net: ConfigNet,
    #[serde(default)]
    pub server: ConfigServer,
    #[serde(default)]
    pub composite: ConfigComposite,
    pub import: Option<Vec<Import>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Import {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub format: String,
    pub path: PathBuf,
    #[serde(flatten)]
    pub rest: Value,
}

/// Define a net composited from many other net
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CompositeName {
    pub name: Option<String>,
    #[serde(flatten)]
    pub composite: CompositeDefaultType,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum Composite {
    Rule(CompositeRule),
}

impl Into<CompositeDefaultType> for Composite {
    fn into(self) -> CompositeDefaultType {
        CompositeDefaultType(self)
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct CompositeDefaultType(pub Composite);

impl<'de> serde::Deserialize<'de> for CompositeDefaultType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        let v = Value::deserialize(deserializer)?;
        match Option::<String>::deserialize(&v["type"]).map_err(de::Error::custom)? {
            Some(_) => {
                let inner = Composite::deserialize(v).map_err(de::Error::custom)?;
                Ok(CompositeDefaultType(inner))
            }
            None => {
                let inner = CompositeRule::deserialize(v).map_err(de::Error::custom)?;
                Ok(CompositeDefaultType(Composite::Rule(inner)))
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Chain {
    One(String),
    Many(Vec<String>),
}

impl Chain {
    pub fn to_vec(self) -> Vec<String> {
        match self {
            Chain::One(s) => vec![s],
            Chain::Many(v) => v,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Net {
    #[serde(rename = "type")]
    pub net_type: String,
    #[serde(default = "local_chain")]
    pub chain: Chain,
    #[serde(flatten)]
    pub rest: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Server {
    #[serde(rename = "type")]
    pub server_type: String,
    #[serde(default = "local_string")]
    pub listen: String,
    #[serde(default = "rule")]
    pub net: String,
    #[serde(flatten)]
    pub rest: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CompositeRuleItem {
    #[serde(rename = "type")]
    pub rule_type: String,
    pub target: String,
    #[serde(flatten)]
    pub rest: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CompositeRule {
    pub rule: Vec<CompositeRuleItem>,
}

pub(crate) fn local_chain() -> Chain {
    Chain::One("local".to_string())
}

fn local_string() -> String {
    "local".to_string()
}

fn rule() -> String {
    "rule".to_string()
}

fn plugins() -> PathBuf {
    PathBuf::from("plugins")
}

impl Config {
    pub async fn post_process(mut self) -> Result<Self> {
        if let Some(imports) = (&mut self).import.take() {
            for i in imports {
                crate::translate::post_process(&mut self, i.clone())
                    .await
                    .context(format!("post process of import: {:?}", i))?;
            }
        }
        Ok(self)
    }
}
