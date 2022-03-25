use crate::deserialize::de_string_to_number;

/// Variant of type of response we expect to use in this application.
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
pub enum VariantResponse {
    /// Cover all auxilary type of response in which to differentiate the type
    /// of response message, we need to check specific field inside structure
    Response(RequestResponse),

    /// Liquidation
    Liquidation(GenericTopic<BybitLiquidationData>),
}

/// Request's response
#[derive(Debug, serde::Deserialize)]
pub struct RequestResponse {
    /// Whether or not subscription is success
    pub success: bool,

    /// Returned message
    pub ret_msg: Option<String>,

    /// Connection id as string
    pub conn_id: String,

    /// Subscribe request object
    pub request: ResponseRequestField,
}

/// Reponse's request field
#[derive(Debug, serde::Deserialize)]
pub struct ResponseRequestField {
    /// Operation
    /// Only this field which can be used to differentiate between type of
    /// response.
    pub op: String,

    /// Arguments
    pub args: Option<Vec<String>>,
}

/// Generic topic as response with varying data for various operation
#[derive(Debug, serde::Deserialize)]
pub struct GenericTopic<T> {
    pub topic: String,
    pub data: GenericData<T>,
}

/// Generic data, just in case if we need to support more streaming-in data
/// structure later e.g. trade.
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
pub enum GenericData<T> {
    Liquidation(T),
}

// we don't need to process anything of this field, thus we don't need to
// convert into specific type e.g. number, but just String for displaying onto
// console, otherwise we can do it with deserialize_with="fn".
/// Bybit's liquidation data
#[derive(Debug, serde::Deserialize)]
pub struct BybitLiquidationData {
    /// Symbol; ticker
    pub symbol: String,

    /// Buy side, or sell side
    pub side: String,
    
    /// Bankruptcy price
    #[serde(deserialize_with = "de_string_to_number")]
    pub price: f64,

    /// Quantity
    #[serde(deserialize_with = "de_string_to_number")]
    pub qty: u32,   // maximum of trading qty depends on asset, but this would be suffice e.g. BTCUSD maxed at 1,000,000

    /// Timestamp in milliseconds
    pub time: u64
}

/// Possible errors as might occur during the operation of the application.
/// Each one contain optional `String` describing more detail for such error.
pub enum OperationError {
    ErrorInternalGeneric(Option<String>),
    ErrorMissingRequiredEnvVar(Option<String>),
    ErrorWssConnect(Option<String>),
    ErrorWssTopicSubscription(Option<String>),
    ErrorInternalSyncCommunication(Option<String>),
}
