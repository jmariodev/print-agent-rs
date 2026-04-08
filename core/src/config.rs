use std::str::FromStr;
use serde::Deserialize;
use anyhow::anyhow;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ambiente {
    Dev,
    Test,
    Prod,
}

impl FromStr for Ambiente {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "dev"  => Ok(Ambiente::Dev),
            "test" => Ok(Ambiente::Test),
            "prod" => Ok(Ambiente::Prod),
            other  => Err(anyhow!("Ambiente inválido: '{}'. Usar dev|test|prod", other)),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub ambiente: Ambiente,
    pub id_cliente: String,
    pub id_punto: String,
    pub broker_url: Option<String>,
    pub broker_port: Option<u16>,
    pub update_url: Option<String>,
    pub log_level: Option<String>,
}

impl Config {
    pub fn topic_subscripcion(&self) -> String {
        let env_str = format!("{:?}", self.ambiente).to_lowercase();
        format!("{}-{}-{}-imp-local", env_str, self.id_cliente, self.id_punto)
    }

    pub fn client_id_mqtt(&self) -> String {
        let env_str = format!("{:?}", self.ambiente).to_lowercase();
        format!("{}-{}-{}", env_str, self.id_cliente, self.id_punto)
    }

    pub fn topic_broadcast_update(&self) -> String {
        let env_str = format!("{:?}", self.ambiente).to_lowercase();
        format!("update-air-{}", env_str)
    }

    pub fn broker_url(&self) -> String {
        let raw = self.broker_url
            .clone()
            .unwrap_or_else(|| "localhost".to_string());

        let mut cleaned = raw.as_str();
        // Remove protocol (mqtt://, wss://, etc)
        if let Some(idx) = cleaned.find("://") {
            cleaned = &cleaned[(idx + 3)..];
        }
        // Remove trailing path if any
        if let Some(idx) = cleaned.find('/') {
            cleaned = &cleaned[..idx];
        }
        // Remove port if included in string (as we use broker_port separately)
        if let Some(idx) = cleaned.find(':') {
            cleaned = &cleaned[..idx];
        }

        cleaned.to_string()
    }

    pub fn broker_port(&self) -> u16 {
        self.broker_port.unwrap_or(1883)
    }

    pub fn update_url_for(&self, target_env: &str) -> String {
        self.update_url
            .clone()
            .unwrap_or_else(|| format!("http://localhost:8000/print-agent/{}/", target_env))
    }
}


