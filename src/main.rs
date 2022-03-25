use tungstenite::Message;
use tungstenite::error::Error as TungsError;
use futures_util::stream::StreamExt;
use futures_util::sink::SinkExt;
use rustelebot::*;
use chrono::{NaiveDateTime, DateTime, Utc};
use separator::Separatable;

use std::time::Duration;

#[macro_use] mod macros;
mod types;
mod deserialize;
mod impls;
mod utils;

use types::*;

#[tokio::main]
async fn main() {
    // create bot instance for telegram
    let telegram_bot_instance = create_instance(
        &match std::env::var("HX_BYBIT_SHIPREKT_TELEGRAM_BOT_TOKEN") {
            Ok(res) => res,
            Err(e) => errprint_exit1!(OperationError::ErrorMissingRequiredEnvVar, "HX_BYBIT_SHIPREKT_TELEGRAM_BOT_TOKEN not defined; err={}", e),
        },
        &match std::env::var("HX_BYBIT_SHIPREKT_TELEGRAM_CHANNEL_CHAT_ID") {
            Ok(res) => res,
            Err(e) => errprint_exit1!(OperationError::ErrorMissingRequiredEnvVar, "HX_BYBIT_SHIPREKT_TELEGRAM_CHANNEL_CHAT_ID not defined; err={}", e),
        });

    // connect to wss
    let (ws_stream, _response) = match utils::connect_async_to_wss("wss://stream.bybit.com/realtime").await {
        Ok(res) => res,
        Err(e) => errprint_exit1!(e),
    };
    println!("Connected to ByBit realtime websocket");

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(30));

    // TODO: provide filtering options through cli e.g. BTCUSD, XRPUSD, etc
    match ws_sender.send(Message::Text(r#"{"op": "subscribe", "args": ["liquidation"]}"#.into())).await {
        Ok(_) => println!("subscribed to liquidation topic"),
        Err(e) => errprint_exit1!(OperationError::ErrorWssTopicSubscription, "error subscribing to liquidation topic; err={}", e),
    }

    loop {
        tokio::select! {
            msg_item = ws_receiver.next() => {
                match msg_item {
                    Some(msg) => {
                        match msg {
                            Ok(Message::Text(json_str)) => {
                                match serde_json::from_str::<'_, VariantResponse>(&json_str) {
                                    Ok(VariantResponse::Response(json_obj)) => {
                                        // TODO: provide option flag at CLI to avoid printing the
                                        // following. Fixed set to false for now.
                                        if false {
                                            // check 'op' field to differentiate type
                                            // of response
                                            match json_obj.request.op.to_lowercase().as_str() {
                                                "ping" => println!("recieved pong msg"),
                                                "subscribe" => println!("received subscribe msg"),
                                                _ => (),
                                            }
                                        }
                                    },
                                    Ok(VariantResponse::Liquidation(json_obj)) => {
                                        let inner_json_obj = match json_obj.data {
                                            GenericData::Liquidation(json_obj) => json_obj,
                                        };

                                        let base_currency = utils::get_base_currency(&inner_json_obj.symbol).unwrap_or("UNKNOWN");
                                        let is_linear = utils::is_linear_perpetual(&inner_json_obj.symbol);
                                        let side = if inner_json_obj.side == "Buy" { "Long" } else { "Short" };

                                        let (ms, ns) = utils::get_ms_and_ns_pair(inner_json_obj.time);
                                        // FIXME: dang, NaiveDateTime::from_timestamp requires i64, this means
                                        // timestamp supports for 132 years further until 2102 since epoch 1970
                                        let datetime: DateTime<Utc> = DateTime::from_utc(NaiveDateTime::from_timestamp(ms as i64, ns), Utc);
                                        let bankruptcy_worth_str = ((inner_json_obj.price * inner_json_obj.qty as f64 * 1000.0_f64).round() / 1000.0_f64).separated_string();
                                        let qty_str = inner_json_obj.qty.separated_string();
                                        let price_str = inner_json_obj.price.separated_string();
                                        let base_or_quote_currency_str = if is_linear { "USDT" } else { base_currency };

                                        let message = format!("Bybit shiprekt a {side} position of {qty} {base_or_quote_currency} (worth ${bankruptcy_value}) on the {symbol} {perpetual_or_not} contract at ${price} - {datetime_str}",
                                            side=side,
                                            qty=qty_str,
                                            base_or_quote_currency=base_or_quote_currency_str,
                                            bankruptcy_value=bankruptcy_worth_str,
                                            symbol=inner_json_obj.symbol,
                                            perpetual_or_not=if utils::is_non_perpetual_contract(&inner_json_obj.symbol) { "Futures" } else { "Perpetual futures" },
                                            price=price_str,
                                            datetime_str=datetime.to_string());

                                        match send_message(&telegram_bot_instance, &message) {
                                            Ok(_) => println!("Notified event: {side} position of {symbol} worth ${bankruptcy_value} with {qty} {base_or_quote_currency} at ${price}",
                                                              symbol=inner_json_obj.symbol,
                                                              side=side,
                                                              bankruptcy_value=bankruptcy_worth_str,
                                                              qty=qty_str,
                                                              base_or_quote_currency=base_or_quote_currency_str,
                                                              price=price_str),
                                            // FIXME: upstream fix for rustelebot for `Display` of `ErrorResult`
                                            Err(e) => eprintln!("{}", e.msg)
                                        }
                                    },
                                    Err(e) => eprintln!("-- error parsing JSON response: {} --", e),
                                }
                            },
                            Ok(Message::Ping(msg)) => println!("Received ping message; msg={:#?}", msg),
                            Ok(Message::Pong(msg)) => println!("Received pong message; msg={:#?}", msg),
                            Ok(Message::Binary(bins)) => println!("Received Binbary message, content={}", std::str::from_utf8(&bins).unwrap_or("unknown")),
                            Ok(Message::Frame(frame)) => println!("Received Frame message, content={:?}", frame),
                            Ok(Message::Close(optional_cf)) => match optional_cf {
                               // no need to pay attention to close frame, it's already closed
                               _ => println!("-- Websocket closed --"),
                            },
                            Err(TungsError::ConnectionClosed) => eprintln!("Error: connection closed"),
                            Err(TungsError::AlreadyClosed) => eprintln!("Error: already closed"),
                            Err(TungsError::Io(e)) => eprintln!("Error: IO; err={}", e),
                            Err(TungsError::Tls(e)) => eprintln!("Error:: Tls error; err={}", e),
                            Err(TungsError::Capacity(e)) => {
                                type CError = tungstenite::error::CapacityError;
                                match e {
                                    CError::TooManyHeaders => eprintln!("Error: CapacityError, too many headers"),
                                    CError::MessageTooLong{ size, max_size } => eprintln!("Error: CapacityError, message too long with size={}, max_size={}", size, max_size),
                                }
                            },
                            Err(TungsError::Protocol(e)) => eprintln!("Error: Protocol, err={}", e),
                            Err(TungsError::SendQueueFull(e)) => {
                                type PMsg = tungstenite::protocol::Message;

                                match e {
                                    PMsg::Text(text) => eprintln!("Error: SendQueueFull for Text message, content={}", text),
                                    PMsg::Binary(bins) => eprintln!("Error: SendQueueFull for Binary message, content={}", std::str::from_utf8(&bins).unwrap_or("unknown")),
                                    PMsg::Ping(bins) => eprintln!("Error: SendQueueFull for Ping message, content={}", std::str::from_utf8(&bins).unwrap_or("unknown")),
                                    PMsg::Pong(bins) => eprintln!("Error: SendQueueFull for Pong message, content={}", std::str::from_utf8(&bins).unwrap_or("unknown")),
                                    PMsg::Close(close_frame_optional) => {
                                        match close_frame_optional {
                                            Some(close_frame) => eprintln!("Error: SendQueueFull for Close message, content={:?}", close_frame),
                                            None => eprintln!("Error: SendQueueFull for Close message, no close-frame content")
                                        }
                                    },
                                    PMsg::Frame(frame) => eprintln!("Error: SendQueueFull for Frame messasge, content={:?}", frame)
                                }
                            },
                            Err(TungsError::Utf8) => eprintln!("Error: Utf8 coding error"),
                            Err(TungsError::Url(e)) => eprintln!("Error: Invalid Url; err={:?}", e),
                            Err(TungsError::Http(e)) => eprintln!("Error: Http error; err={:?}", e),
                            Err(TungsError::HttpFormat(e)) => eprintln!("Error: Http format error; err{:?}", e),
                        }
                    },
                    None => (),
                }
            }
            _ = heartbeat_interval.tick() => {
                match ws_sender.send(Message::Text(r#"{"op":"ping"}"#.into())).await {
                    Ok(_) => println!("send ping message"),
                    Err(e) => eprintln!("error sending ping message; err={}", e),
                }
            }
        }
    }
}
