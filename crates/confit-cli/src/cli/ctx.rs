use std::collections::HashMap;

pub struct Ctx {
    vars: HashMap<String, String>,
}

impl Ctx {
    pub fn from_cwd(set_vars: &[String]) -> Result<Self, CtxError> {
        let vars = parse_vars(set_vars)?;
        Ok(Ctx { vars })
    }

    pub fn vars(&self) -> &HashMap<String, String> {
        &self.vars
    }
}

fn parse_vars(set_vars: &[String]) -> Result<HashMap<String, String>, CtxError> {
    let mut vars = HashMap::new();
    for item in set_vars {
        let (k, v) = item
            .split_once('=')
            .ok_or_else(|| CtxError::BadVar(item.clone()))?;
        vars.insert(k.to_string(), v.to_string());
    }
    Ok(vars)
}

#[derive(Debug, thiserror::Error)]
pub enum CtxError {
    #[error("expected KEY=VALUE, got '{0}'")]
    BadVar(String),
}
