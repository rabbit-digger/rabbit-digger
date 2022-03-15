use rd_interface::{config::Config, prelude::*, registry::JsonSchema};
use serde::{Deserialize, Serialize};

#[derive(JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VecStr {
    Single(String),
    Vec(Vec<String>),
}

impl VecStr {
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        Iter {
            inner: self,
            index: 0,
        }
    }
    pub fn into_vec(self) -> Vec<String> {
        match self {
            VecStr::Single(t) => vec![t],
            VecStr::Vec(v) => v,
        }
    }
}

impl From<Vec<String>> for VecStr {
    fn from(x: Vec<String>) -> Self {
        VecStr::Vec(x)
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
    type Item = &'a String;

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
                let i = x.get(self.index);
                self.index += 1;
                i
            }
        }
    }
}
