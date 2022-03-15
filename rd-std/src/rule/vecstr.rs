use rd_interface::{config::Config, prelude::*, registry::JsonSchema};
use serde::{Deserialize, Serialize};
use vec_strings::Strings;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MyStrings {
    #[serde(flatten)]
    inner: Strings,
}

impl JsonSchema for MyStrings {
    fn schema_name() -> String {
        "Strings".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        gen.subschema_for::<Vec<String>>()
    }
}

#[derive(JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VecStr {
    Single(String),
    Vec(MyStrings),
}

impl VecStr {
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        Iter {
            inner: self,
            index: 0,
        }
    }
    pub fn into_vec(self) -> Vec<String> {
        match self {
            VecStr::Single(t) => vec![t],
            VecStr::Vec(v) => v.inner.into_iter().map(|i| i.to_string()).collect(),
        }
    }
}

impl From<Vec<String>> for VecStr {
    fn from(x: Vec<String>) -> Self {
        let mut r = Strings::new();
        for i in x {
            r.push(&i);
        }
        VecStr::Vec(MyStrings { inner: r })
    }
}

impl Config for VecStr {
    fn visit(
        &mut self,
        ctx: &mut rd_interface::config::VisitorContext,
        visitor: &mut dyn rd_interface::config::Visitor,
    ) -> rd_interface::Result<()> {
        Ok(())
    }
}

pub struct Iter<'a> {
    inner: &'a VecStr,
    index: usize,
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner {
            VecStr::Single(x) => {
                if self.index == 0 {
                    self.index += 1;
                    Some(x)
                } else {
                    None
                }
            }
            VecStr::Vec(x) => {
                let i = x.inner.get(self.index as u32);
                self.index += 1;
                i
            }
        }
    }
}
