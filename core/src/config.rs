use std::str::FromStr;
use serde::Deserialize;
use anyhow::anyhow;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub struct Ambiente(pub String);

impl TryFrom<String> for Ambiente {
    type Error = anyhow::Error;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        let lower = s.to_lowercase();
        if lower == "dev" || lower == "test" || lower.starts_with("prod") {
            Ok(Ambiente(lower))
        } else {
            Err(anyhow!("Ambiente inválido: '{}'. Usar dev|test|prod|prod_*", s))
        }
    }
}

impl FromStr for Ambiente {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.to_string().try_into()
    }
}

impl Ambiente {
    pub fn is_prod(&self) -> bool {
        self.0.starts_with("prod")
    }

    pub fn is_dev_or_test(&self) -> bool {
        self.0 == "dev" || self.0 == "test"
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn base_env(&self) -> &str {
        if self.is_prod() {
            "prod"
        } else {
            self.as_str()
        }
    }
}

impl std::fmt::Display for Ambiente {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
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
        format!("{}-{}-{}-imp-local", self.ambiente.as_str(), self.id_cliente, self.id_punto)
    }

    pub fn client_id_mqtt(&self) -> String {
        format!("{}-{}-{}", self.ambiente.as_str(), self.id_cliente, self.id_punto)
    }

    pub fn topic_broadcast_update(&self) -> String {
        format!("update-air-{}", self.ambiente.base_env())
    }

    pub fn is_wss(&self) -> bool {
        let raw = self.broker_url
            .clone()
            .unwrap_or_else(|| "wss://gd5.gamasoftcol.com".to_string());
        raw.starts_with("wss://")
    }

    pub fn broker_url(&self) -> String {
        let raw = self.broker_url
            .clone()
            .unwrap_or_else(|| "wss://gd5.gamasoftcol.com".to_string());

        if raw.starts_with("wss://") || raw.starts_with("ws://") {
            // async-tungstenite necesita la URL COMPLETA: scheme://host:puerto/path
            let (scheme, rest) = if let Some(r) = raw.strip_prefix("wss://") {
                ("wss", r)
            } else {
                ("ws", raw.strip_prefix("ws://").unwrap_or(&raw))
            };

            // Separar host[:puerto] del /path
            let (host_port, path) = if let Some(idx) = rest.find('/') {
                (&rest[..idx], &rest[idx..])
            } else {
                (rest, "")
            };

            // Separar host y puerto (si el usuario lo embebió en la URL)
            let (host, port_in_url) = if let Some(idx) = host_port.find(':') {
                (&host_port[..idx], Some(&host_port[idx + 1..]))
            } else {
                (host_port, None)
            };

            // Puerto efectivo: URL embebido > broker_port > 1883 por defecto
            let port = port_in_url
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or_else(|| self.broker_port.unwrap_or(1883));

            // Path efectivo: si no hay path, EMQX usa /mqtt por convención
            let effective_path = if path.is_empty() { "/mqtt" } else { path };

            // URL final con puerto embebido tal como lo exige async-tungstenite
            return format!("{}://{}:{}{}", scheme, host, port, effective_path);
        }

        // Conexión TCP pura: Rumqttc exige estrictamente SOLO el dominio crudo.
        let mut cleaned = raw.as_str();
        if let Some(idx) = cleaned.find("://") {
            cleaned = &cleaned[(idx + 3)..];
        }
        if let Some(idx) = cleaned.find('/') {
            cleaned = &cleaned[..idx];
        }
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
            .unwrap_or_else(|| format!("https://www.gamasoftcol.com/ActualizadorAIR/{}/", target_env))
    }
}


