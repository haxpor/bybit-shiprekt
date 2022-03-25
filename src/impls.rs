use crate::types::OperationError;
use std::fmt::{Display, Formatter, Error};

impl Display for OperationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        type OptErr = OperationError;

        match self {
            OptErr::ErrorInternalGeneric(opt_msg) => {
                match opt_msg {
                    Some(msg) => write!(f, "error internal generic; {}", msg),
                    None => write!(f, "error internal generic")
                }
            },
            OptErr::ErrorMissingRequiredEnvVar(opt_msg) => {
                match opt_msg {
                    Some(msg) => write!(f, "error missing required environment variable; {}", msg),
                    None => write!(f, "error missing required environment variable")
                }
            },
            OptErr::ErrorWssConnect(opt_msg) => {
                match opt_msg {
                    Some(msg) => write!(f, "error connecting to WSS; {}", msg),
                    None => write!(f, "error connecting to WSS;")
                }
            },
            OptErr::ErrorWssTopicSubscription(opt_msg) => {
                match opt_msg {
                    Some(msg) => write!(f, "error subscribing to a topic of websocket; {}", msg),
                    None => write!(f, "error subscribing to a topic of websocket")
                }
            },
            OptErr::ErrorInternalSyncCommunication(opt_msg) => {
                match opt_msg {
                    Some(msg) => write!(f, "error in internal syncing-communication mechanism; {}", msg),
                    None => write!(f, "error in internal syncing-communication mechanism")
                }
            }
        }
    }
}
