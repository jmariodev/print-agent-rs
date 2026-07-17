use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "action")]
pub enum Comando {
    #[serde(rename = "listPrinters")]
    ListPrinters {
        #[serde(rename = "responseTopic")]
        response_topic: String,
    },
    #[serde(rename = "print")]
    Print {
        #[serde(rename = "responseTopic")]
        response_topic: String,
        #[serde(rename = "printerName")]
        printer_name: String,
        #[serde(rename = "fileToPrint")]
        file_to_print: String,
    },
    #[serde(rename = "update-air")]
    UpdateAir {
        #[serde(rename = "responseTopic")]
        response_topic: String,
        ambiente: Option<String>,
    },
    #[serde(rename = "ping")]
    Ping {
        #[serde(rename = "responseTopic")]
        response_topic: String,
    },
}

#[derive(Deserialize)]
pub struct FallbackComando {
    #[serde(rename = "responseTopic")]
    pub response_topic: Option<String>,
}
