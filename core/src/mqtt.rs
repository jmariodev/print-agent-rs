use rumqttc::{AsyncClient, MqttOptions, QoS, Event, Incoming};
use anyhow::Result;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use crate::{config::Config, traits::Plataforma, messages::*};
use base64::{Engine, engine::general_purpose::STANDARD as B64};

pub async fn run(
    cfg: Config,
    plataforma: Arc<dyn Plataforma>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    let mut opts = MqttOptions::new(cfg.client_id_mqtt(), cfg.broker_url(), cfg.broker_port());
    // Aumentar el límite de payload (20 MB de entrada y salida) para recibir PDFs pesados
    opts.set_max_packet_size(20 * 1024 * 1024, 20 * 1024 * 1024);
    let (client, mut event_loop) = AsyncClient::new(opts, 10);

    let topico_suscripcion = cfg.topic_subscripcion();
    let topico_broadcast = cfg.topic_broadcast_update();

    loop {
        tokio::select! {
            event = event_loop.poll() => {
                match event {
                    Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                        let c = client.clone();
                        let ts = topico_suscripcion.clone();
                        let tb = topico_broadcast.clone();
                        tokio::spawn(async move {
                            tracing::info!("Conexión establecida con el broker. Iniciando/Restaurando suscripciones...");
                            if let Err(e) = c.subscribe(&ts, QoS::AtLeastOnce).await {
                                tracing::error!("Error suscribiendo a {}: {}", ts, e);
                            } else {
                                tracing::info!("Suscrito a tópico principal: {}", ts);
                            }
                            
                            if let Err(e) = c.subscribe(&tb, QoS::AtLeastOnce).await {
                                tracing::error!("Error suscribiendo a {}: {}", tb, e);
                            } else {
                                tracing::info!("Suscrito a tópico broadcast (silencioso): {}", tb);
                            }
                        });
                    }
                    Ok(Event::Incoming(Incoming::Publish(p))) => {
                        let plataforma = Arc::clone(&plataforma);
                        let client = client.clone();
                        let cfg = cfg.clone();

                        tokio::spawn(async move {
                            manejar_mensaje(p.topic, p.payload.to_vec(), &client, plataforma, &cfg).await;
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Error MQTT al intentar conectar a broker {} (reconectando): {}", cfg.broker_url(), e);
                        // rumqttc tiene su propio backoff de reconexión, pero evitamos un tight loop si la red cae brusca
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    }
                    _ => {}
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    tracing::info!("Señal de shutdown recibida, cerrando MQTT.");
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn manejar_mensaje(
    topic: String,
    payload: Vec<u8>,
    client: &AsyncClient,
    plataforma: Arc<dyn Plataforma>,
    cfg: &Config,
) {
    // 1. Verificar si es el tópico de broadcast update masivo
    if topic == cfg.topic_broadcast_update() {
        tracing::info!("Recibido comando BROADCAST de actualización por tópico silencioso.");
        
        let env_str = format!("{:?}", cfg.ambiente).to_lowercase();
        let update_url = cfg.update_url_for(&env_str);
        tokio::spawn(async move {
            const VERSION_ACTUAL: &str = env!("CARGO_PKG_VERSION");
            tracing::info!("Iniciando comprobación de red silenciosa a {}", update_url);
            if let Err(e) = crate::updater::verificar_y_descargar(&update_url, VERSION_ACTUAL).await {
                tracing::warn!("Proceso de actualización broadcast abortado/fallido: {}", e);
            }
        });
        return;
    }

    // 2. Comportamiento normal JSON entrante (Comando transaccional)
    let respuesta_texto = match serde_json::from_slice::<Comando>(&payload) {
        Err(e) => {
            tracing::warn!("JSON inválido: {}", e);
            // Intentar extraer el responseTopic mínimamente para avisar al emisor local
            if let Ok(fallback) = serde_json::from_slice::<FallbackComando>(&payload) {
                if let Some(topic) = fallback.response_topic {
                    if let Err(e2) = client.publish(&topic, QoS::AtLeastOnce, false, "Action is required".as_bytes()).await {
                        tracing::error!("Error publicando respuesta fallback a {}: {}", topic, e2);
                    }
                } else {
                    tracing::error!("Comando inválido sin responseTopic.");
                }
            }
            return;
        }
        Ok(cmd) => procesar_comando(cmd, plataforma, cfg).await,
    };

    if let Some((resp_topic, resp_payload)) = respuesta_texto {
        if let Err(e) = client
            .publish(&resp_topic, QoS::AtLeastOnce, false, resp_payload.as_bytes())
            .await
        {
            tracing::error!("No se pudo publicar respuesta MQTT en {}: {}", resp_topic, e);
        }
    }
}

async fn procesar_comando(cmd: Comando, plataforma: Arc<dyn Plataforma>, cfg: &Config) -> Option<(String, String)> {
    match cmd {
        Comando::ListPrinters { response_topic } => {
            match plataforma.listar_impresoras().await {
                Ok(printers) => {
                    let text = format!("[{}]", printers.join(", "));
                    Some((response_topic, text))
                }
                Err(_) => Some((response_topic, "Error listando impresoras".into())),
            }
        }

        Comando::Print { response_topic, printer_name, file_to_print } => {
            let resultado = imprimir_pdf(printer_name, file_to_print, plataforma).await;
            match resultado {
                Ok(_) => Some((response_topic, "Impresion exitosa".into())),
                Err(_) => Some((response_topic, "Error al imprimir".into())),
            }
        }

        Comando::UpdateAir { response_topic, ambiente } => {
            let config_ambiente = format!("{:?}", cfg.ambiente).to_lowercase();
            let env_solicitado = ambiente.unwrap_or(config_ambiente.clone());
            
            // Logica simulando actualización, enviamos la respuesta asincrónica y luego ejecutamos.
            let resp_tuple = Some((response_topic, "Verificando actualizaciones".into()));
            
            // Retrasar disparo update
            let update_url = cfg.update_url_for(&env_solicitado);
            tokio::spawn(async move {
                const VERSION_ACTUAL: &str = env!("CARGO_PKG_VERSION");
                tracing::info!("Comprobando actualización para el ambiente: {}", env_solicitado);
                if let Err(e) = crate::updater::verificar_y_descargar(&update_url, VERSION_ACTUAL).await {
                    tracing::warn!("Error fallido en UpdateAir transaccional: {}", e);
                }
            });
            
            resp_tuple
        }
    }
}

async fn imprimir_pdf(
    printer_name: String,
    pdf_base64: String,
    plataforma: Arc<dyn Plataforma>,
) -> Result<()> {
    // Decodificar base64
    let bytes = B64.decode(&pdf_base64)
        .map_err(|e| anyhow::anyhow!("base64 inválido: {}", e))?;

    // Generar un ID temporal para el job usando la hora actual para no colisionar en disco
    let job_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    tokio::fs::create_dir_all("temp").await?;
    let ruta = format!("temp/{}.pdf", job_id);
    tokio::fs::write(&ruta, &bytes).await?;

    let ruta_clone = ruta.clone();
    scopeguard::defer! {
        if let Err(e) = std::fs::remove_file(&ruta_clone) {
             tracing::warn!("No se pudo eliminar PDF temporal {}: {}", ruta_clone, e);
        }
    };

    plataforma.imprimir(&printer_name, &ruta).await?;

    Ok(())
}
