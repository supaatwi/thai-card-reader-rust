use card_reader::thai_card_apdu_command::ThaiCommandAPDU;
use futures_util::{SinkExt, StreamExt};
use pcsc::{Card, Context, Protocols, Scope, ShareMode, State, MAX_BUFFER_SIZE, Error as PCSCError};
use std::{error::Error, ffi::CStr, thread, time::Duration};
use encoding_rs::WINDOWS_874;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::accept_async;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct CitizenData {
    #[serde(rename = "cid")]
    citizen_id: String,
    #[serde(rename = "fullNameTh")]
    fullname_th: String,
    #[serde(rename = "fullNameEn")]
    fullname_en: String,
    #[serde(rename = "dateOfBirth")]
    date_of_birth: String,
    #[serde(rename = "genderId")]
    gender: String,
    #[serde(rename = "cardIssue")]
    card_issue: String,
    #[serde(rename = "issueDate")]
    issue_date: String,
    #[serde(rename = "expireDate")]
    expire_date: String,
    address: String,
    
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let context: Context = Context::establish(Scope::Terminal)?;
    let (tx, _) = broadcast::channel::<CitizenData>(10);

    tokio::task::spawn(start_websocket_server(tx.clone()));
    
    let mut readers_buf = [0; 2048];
    let readers: pcsc::ReaderNames<'_> = context.list_readers(&mut readers_buf)?;

    println!("Available readers:");
    let mut reader: Option<&CStr> = None;
    for (i, r) in readers.into_iter().enumerate() {
        println!("- {}", r.to_string_lossy());
        if i == 0 {
            reader = Some(r);
        }
    }

    if let Some(reader) = reader {
        println!("\nMonitoring reader: {}", reader.to_string_lossy());
        monitor_card_events(&context, reader, tx)?;
    } else {
        println!("No card readers found!");
    }

    Ok(())
}



async fn start_websocket_server(tx: broadcast::Sender<CitizenData>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:9982").await?;
    println!("WebSocket server listening on ws://127.0.0.1:9982");

    while let Ok((stream, _)) = listener.accept().await {
        let tx = tx.clone();
        tokio::spawn(async move {
            let ws_stream = accept_async(stream).await.expect("Error during WebSocket handshake");
            let mut rx = tx.subscribe(); // Each client gets a separate receiver
            let (mut write, _) = ws_stream.split();

            while let Ok(citizen_data) = rx.recv().await {
                // Send a message indicating card insertion
                let message = serde_json::to_string(&citizen_data).unwrap();
                write.send(Message::Text(message)).await.unwrap();
            }
        });
    }
    Ok(())
}

fn monitor_card_events(context: &Context, reader: &CStr, tx: broadcast::Sender<CitizenData>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut reader_states = vec![
        pcsc::ReaderState::new(reader, State::UNAWARE)
    ];

    let mut last_state = State::UNAWARE;
    const MAX_RETRIES: u8 = 3;
    
    loop {
        match context.get_status_change(Some(Duration::from_millis(100)), &mut reader_states) {
            Ok(_) => {
                let current_state = reader_states[0].event_state();
                
                if current_state != last_state {
                    if current_state.contains(State::PRESENT) && !last_state.contains(State::PRESENT) {
                        println!("Card inserted!");
                        
                        for retry in 0..MAX_RETRIES {
                            if retry > 0 {
                                println!("Retrying connection... (attempt {})", retry + 1);
                                thread::sleep(Duration::from_millis(500));
                            }
                            
                            match connect_and_read_card(context, reader) {
                                Ok(citizen_data) => {
                                    println!("{:#?}", citizen_data);
                                    let _ = tx.send(citizen_data);
                                    break;
                                }
                                Err(e) => {
                                    println!("Error reading card data (attempt {}): {}", retry + 1, e);
                                    if retry == MAX_RETRIES - 1 {
                                        println!("Failed to read card after {} attempts", MAX_RETRIES);
                                    }
                                }
                            }
                        }
                    }
                    
                    if !current_state.contains(State::PRESENT) && last_state.contains(State::PRESENT) {
                        println!("Card removed!");
                    }
                }

                last_state = current_state;
                reader_states[0].sync_current_state();
            }
            Err(PCSCError::NoService) => {
                println!("PC/SC service not running");
                thread::sleep(Duration::from_secs(1));
            }
            Err(PCSCError::Timeout) => continue,
            Err(e) => {
                println!("Error monitoring card: {}", e);
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

fn connect_and_read_card(context: &Context, reader: &CStr) -> Result<CitizenData, Box<dyn Error>> {
    let card = context.connect(reader, ShareMode::Shared, Protocols::ANY)?;
    read_citizen_data(&card)
}

fn read_citizen_data(card: &Card) -> Result<CitizenData, Box<dyn Error>> {
    let mut response_buf = [0; MAX_BUFFER_SIZE];
    
    // SELECT command with retry
    let mut select_attempts = 0;
    while select_attempts < 3 {
        match card.transmit(&ThaiCommandAPDU::APDU_SELECT, &mut response_buf) {
            Ok(_) => break,
            Err(e) => {
                select_attempts += 1;
                if select_attempts >= 3 {
                    return Err(Box::new(e));
                }
                thread::sleep(Duration::from_millis(200));
            }
        }
    }

    let mut citizen_data = CitizenData::default();
    
    // Helper function to read data with retry
    fn read_data_with_retry(card: &Card, command: &[u8]) -> Result<String, Box<dyn Error>> {
        let mut bytes = [0; MAX_BUFFER_SIZE];
        let mut attempts = 0;
        while attempts < 3 {
            match card.transmit(command, &mut bytes) {
                Ok(_) => {
                    let (cow, _, _) = WINDOWS_874.decode(&bytes);
                    return Ok(clear_byte(cow.to_string()));
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= 3 {
                        return Err(Box::new(e));
                    }
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }
        Err("Failed to read data after maximum retries".into())
    }

    // Read each field with retry
    citizen_data.citizen_id = read_data_with_retry(card, &ThaiCommandAPDU::APDU_CID)?;
    citizen_data.fullname_th = read_data_with_retry(card, &ThaiCommandAPDU::APDU_FULLNAME_TH)?;
    citizen_data.fullname_en = read_data_with_retry(card, &ThaiCommandAPDU::APDU_FULLNAME_EN)?;
    citizen_data.date_of_birth = read_data_with_retry(card, &ThaiCommandAPDU::APDU_DATE_OF_BIRTH)?;
    citizen_data.gender = read_data_with_retry(card, &ThaiCommandAPDU::APDU_GENDER)?;
    citizen_data.card_issue = read_data_with_retry(card, &ThaiCommandAPDU::APDU_CARD_ISSUE)?;
    citizen_data.issue_date = read_data_with_retry(card, &ThaiCommandAPDU::APDU_ISSUE_DATE)?;
    citizen_data.expire_date = read_data_with_retry(card, &ThaiCommandAPDU::APDU_EXPIRE_DATE)?;
    citizen_data.address = read_data_with_retry(card, &ThaiCommandAPDU::APDU_ADDRESS)?;
    
    Ok(citizen_data)
}

fn clear_byte(value: String) -> String {
    value.chars()
    .filter(|c| !c.is_control())
    .collect::<String>().replace(" ", "").replace("#", " ")
}