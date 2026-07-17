use crate::config::Config;
use anyhow::Result;
use rumqttc::{AsyncClient, MqttOptions, QoS, Event, Incoming};
use std::time::Duration;
use uuid::Uuid;
use std::sync::Arc;
use rustls::client::danger::{ServerCertVerifier, HandshakeSignatureValid, ServerCertVerified};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

use crate::messages::Comando;

#[derive(Debug)]
pub struct InsecureCertVerifier;

impl ServerCertVerifier for InsecureCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA1,
            SignatureScheme::ECDSA_SHA1_Legacy,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}

pub async fn verificar_agente_activo(ambiente_str: String, cliente: String, punto: String) -> Result<bool> {
    let cfg = Config {
        ambiente: ambiente_str.parse()?,
        id_cliente: cliente.clone(),
        id_punto: punto.clone(),
        broker_url: None,
        broker_port: None,
        update_url: None,
        log_level: None,
    };

    let client_id = format!("verify-{}-{}", cfg.client_id_mqtt(), Uuid::new_v4().as_simple());
    let mut opts = MqttOptions::new(&client_id, cfg.broker_url(), cfg.broker_port());
    
    if cfg.is_wss() {
        let config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(InsecureCertVerifier))
            .with_no_client_auth();
            
        opts.set_transport(rumqttc::Transport::Wss(rumqttc::TlsConfiguration::Rustls(Arc::new(config))));
    }

    let (client, mut event_loop) = AsyncClient::new(opts, 10);
    
    let topic_tx = cfg.topic_subscripcion();
    let response_topic = format!("respuestas/verify-{}", Uuid::new_v4().as_simple());

    tracing::info!("Verificando agente activo para ambiente {}, cliente {}, punto {}", ambiente_str, cliente, punto);
    let timeout_duration = Duration::from_secs(5); //Tiempo que vamos a esperar para que los air respondan si estan activos en ese topic
    
    let result = tokio::time::timeout(timeout_duration, async {
        loop {
            match event_loop.poll().await {
                Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                    client.subscribe(&response_topic, QoS::AtLeastOnce).await?;
                }
                Ok(Event::Incoming(Incoming::SubAck(_))) => {
                    let ping_cmd = Comando::Ping {
                        response_topic: response_topic.clone(),
                    };
                    let payload = serde_json::to_string(&ping_cmd)?;
                    client.publish(&topic_tx, QoS::AtLeastOnce, false, payload.as_bytes()).await?;
                }
                Ok(Event::Incoming(Incoming::Publish(p))) => {
                    if p.topic == response_topic {
                        let payload_str = String::from_utf8_lossy(&p.payload);
                        if payload_str == "pong" || payload_str == "Action is required" {
                            return Ok(true);
                        }
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Error MQTT: {}", e));
                }
                _ => {}
            }
        }
    }).await;

    let _ = client.disconnect().await;

    match result {
        Ok(Ok(activo)) => Ok(activo), 
        Ok(Err(_)) => Ok(false),
        Err(_) => Ok(false), 
    }
}
