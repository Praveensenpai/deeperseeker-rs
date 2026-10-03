# CODEBASE.md: deeperseeker Semantic Digest — v0.2.14

> **Notice**: AI-optimized semantic index. Do not write narrative prose. Keep token density high.

## 1. System Topology & Data Flow
```text
Entrypoint ──> CLI/Parser ──> Domain Logic ──> Infra/IO
```

## 2. Global Constraints & Architecture Patterns
- **Primary Language**: Rust 2021 edition
- **Architectural Paradigm**: Role-based (domain/, infra/, api/cli/, tui/)
- **Hard Constraints**: <400 lines/file, <60 lines/fn, zero production unwrap(), 0 warnings.
- **Target Distribution**: Linux x86_64 standalone binary

## 3. Module & Interface Skeleton

### `src/api/anthropic.rs` (Role: api, Lines: 145)
- **Responsibility**: Core api logic in src/api/anthropic.rs
- **Imports**: use crate :: api :: chat :: chat_completions , use crate :: api :: state :: AppState , use crate :: domain :: anthropic :: { AnthropicBlock , AnthropicContent , AnthropicMessage , AnthropicMessageRequest , AnthropicMessageResponse , AnthropicUsage , } , use crate :: domain :: openai :: { ChatCompletionRequest , ChatCompletionResponse , ChatMessage , ContentPart , MessageContent , } , use axum :: { extract :: State , http :: StatusCode , response :: { IntoResponse , Response } , Json , } , use serde_json :: json 
- **Public Functions & Signatures**:
  ```rust
  async fn anthropic_messages (State (state) : State < AppState > , Json (req) : Json < AnthropicMessageRequest > ,) -> Result < Response , (StatusCode , Json < serde_json :: Value >) >
  ```

### `src/api/chat.rs` (Role: api, Lines: 246)
- **Responsibility**: Core api logic in src/api/chat.rs
- **Imports**: use crate :: api :: chat_stream :: { handle_streaming_response , handle_unary_response } , use crate :: api :: state :: AppState , use crate :: domain :: openai :: ChatCompletionRequest , use crate :: domain :: session :: compute_signature , use crate :: domain :: token :: Token , use crate :: infra :: db :: { find_session , mark_active , mark_limited , pick_token , touch_token } , use crate :: infra :: deepseek_client :: CompletionArgs , use crate :: infra :: media :: { resolve_message_media , MediaContext } , use crate :: infra :: prompt :: build_prompt_for_turn , use axum :: { extract :: State , http :: StatusCode , response :: Response , Json } , use serde_json :: json 
- **Public Functions & Signatures**:
  ```rust
  async fn chat_completions (State (state) : State < AppState > , Json (req) : Json < ChatCompletionRequest > ,) -> Result < Response , (StatusCode , Json < serde_json :: Value >) >
  ```

### `src/api/chat_chunks.rs` (Role: api, Lines: 180)
- **Responsibility**: Core api logic in src/api/chat_chunks.rs
- **Imports**: use crate :: domain :: openai :: { ChatChoice , ChatCompletionChunk , ChatCompletionResponse , ChunkChoice , ChunkDelta , ResponseMessage , ToolCall , Usage , } , use crate :: infra :: sse :: ExtractedChunk , use std :: time :: { SystemTime , UNIX_EPOCH } , use uuid :: Uuid 
- **Types & Enums**:
  ```rust
  pub struct ChatResponseArgs
  ```
- **Public Functions & Signatures**:
  ```rust
  fn build_chat_response (args : ChatResponseArgs) -> ChatCompletionResponse
  fn create_openai_chunks (chat_id : & str , created : u64 , model : & str , chunks : Vec < ExtractedChunk > ,) -> Vec < ChatCompletionChunk >
  fn make_text_chunk (chat_id : & str , created : u64 , model : & str , text : & str ,) -> ChatCompletionChunk
  fn make_reasoning_chunk (chat_id : & str , created : u64 , model : & str , reasoning : & str ,) -> ChatCompletionChunk
  fn make_tool_calls_chunk (chat_id : & str , created : u64 , model : & str , tool_calls : Vec < ToolCall > ,) -> ChatCompletionChunk
  fn build_terminal_chunk (chat_id : & str , created : u64 , model : & str , prompt_tokens : u32 , comp_tokens : u32 , finish_reason : & str ,) -> Option < String >
  fn current_timestamp () -> u64
  ```

### `src/api/chat_stream.rs` (Role: api, Lines: 363)
- **Responsibility**: Core api logic in src/api/chat_stream.rs
- **Imports**: use crate :: api :: chat_chunks :: { build_chat_response , build_terminal_chunk , create_openai_chunks , current_timestamp , make_reasoning_chunk , make_text_chunk , make_tool_calls_chunk , ChatResponseArgs , } , use crate :: api :: state :: AppState , use crate :: domain :: openai :: ChatMessage , use crate :: domain :: session :: { compute_next_signature , next_parent_id , Session } , use crate :: infra :: db :: save_session , use crate :: infra :: dsml :: { find_dsml_block_start , parse_dsml , safe_unambiguous_len , ParsedDsml } , pub use crate :: infra :: sse :: { drain_sse_lines , extract_chunks_from_event , parse_sse_line , ExtractedChunk , SseLineResult , } , use crate :: infra :: usage_db :: record_usage , use axum :: { body :: Body , http :: { header :: CONTENT_TYPE , StatusCode } , response :: { IntoResponse , Response } , Json , } , use futures :: StreamExt , use uuid :: Uuid 
- **Public Functions & Signatures**:
  ```rust
  async fn handle_streaming_response (state : & AppState , model : String , token_id : i64 , session_id : String , parent_id : i64 , req_messages : Vec < ChatMessage > , upstream_resp : reqwest :: Response ,) -> Result < Response , (StatusCode , String) >
  async fn handle_unary_response (state : & AppState , model : String , token_id : i64 , session_id : String , parent_id : i64 , req_messages : & [ChatMessage] , upstream_resp : reqwest :: Response ,) -> Result < Response , (StatusCode , String) >
  ```

### `src/api/dashboard.rs` (Role: api, Lines: 203)
- **Responsibility**: Core api logic in src/api/dashboard.rs
- **Imports**: use crate :: api :: state :: AppState , use crate :: domain :: usage :: format_metric , use crate :: infra :: db :: { add_token as db_add_token , delete_token as db_delete_token , get_tokens } , use axum :: { extract :: { Form , Path , State } , http :: { header :: { COOKIE , SET_COOKIE } , HeaderMap , StatusCode , } , response :: { Html , IntoResponse , Redirect , Response } , } , use serde :: { Deserialize , Serialize } , use sha2 :: { Digest , Sha256 } , use tera :: Context 
- **Types & Enums**:
  ```rust
  pub struct LoginForm
  pub struct AddTokenForm
  pub struct DashboardTokenView
  pub struct DashboardSummaryView
  ```
- **Public Functions & Signatures**:
  ```rust
  async fn show_login (State (state) : State < AppState >) -> Response
  async fn submit_login (State (state) : State < AppState > , Form (form) : Form < LoginForm >) -> Response
  async fn logout () -> Response
  async fn show_dashboard (State (state) : State < AppState > , headers : HeaderMap) -> Response
  async fn add_token (State (state) : State < AppState > , headers : HeaderMap , Form (form) : Form < AddTokenForm > ,) -> Response
  async fn delete_token (State (state) : State < AppState > , headers : HeaderMap , Path (token_id) : Path < i64 > ,) -> Response
  ```

### `src/api/files.rs` (Role: api, Lines: 124)
- **Responsibility**: Core api logic in src/api/files.rs
- **Imports**: use crate :: api :: state :: AppState , use crate :: infra :: db :: { pick_token , record_file } , use axum :: { extract :: { Multipart , State } , http :: StatusCode , Json , } , use serde_json :: json , use std :: time :: { SystemTime , UNIX_EPOCH } 
- **Public Functions & Signatures**:
  ```rust
  async fn upload_file_openai (State (state) : State < AppState > , mut multipart : Multipart ,) -> Result < Json < serde_json :: Value > , (StatusCode , Json < serde_json :: Value >) >
  async fn upload_file_anthropic (State (state) : State < AppState > , multipart : Multipart ,) -> Result < Json < serde_json :: Value > , (StatusCode , Json < serde_json :: Value >) >
  ```

### `src/api/health.rs` (Role: api, Lines: 29)
- **Responsibility**: Core api logic in src/api/health.rs
- **Imports**: use crate :: api :: state :: AppState , use crate :: infra :: db :: get_tokens , use axum :: { extract :: State , response :: { IntoResponse , Redirect } , Json , } , use serde_json :: json 
- **Public Functions & Signatures**:
  ```rust
  async fn root () -> impl IntoResponse
  async fn health (State (state) : State < AppState >) -> impl IntoResponse
  ```

### `src/api/middleware.rs` (Role: api, Lines: 47)
- **Responsibility**: Core api logic in src/api/middleware.rs
- **Imports**: use crate :: api :: state :: AppState , use axum :: { extract :: { Request , State } , http :: { header :: AUTHORIZATION , StatusCode } , middleware :: Next , response :: { IntoResponse , Response } , Json , } , use serde_json :: json 
- **Public Functions & Signatures**:
  ```rust
  async fn require_api_key (State (state) : State < AppState > , req : Request , next : Next) -> Response
  ```

### `src/api/models.rs` (Role: api, Lines: 31)
- **Responsibility**: Core api logic in src/api/models.rs
- **Imports**: use crate :: domain :: openai :: { ModelList , ModelObject } , use axum :: { response :: IntoResponse , Json } 
- **Public Functions & Signatures**:
  ```rust
  async fn list_models () -> impl IntoResponse
  ```

### `src/api/state.rs` (Role: api, Lines: 40)
- **Responsibility**: Core api logic in src/api/state.rs
- **Imports**: use crate :: config :: AppConfig , use crate :: infra :: deepseek_client :: DeepSeekClient , use crate :: infra :: pow :: PowSolver , use std :: collections :: HashMap , use std :: sync :: Arc , use tera :: Tera , use tokio :: sync :: Mutex , use tokio_rusqlite :: Connection 
- **Types & Enums**:
  ```rust
  pub struct AppState
  ```
- **Public Functions & Signatures**:
  ```rust
  async fn increment_in_flight (& self , token_id : i64)
  async fn decrement_in_flight (& self , token_id : i64)
  async fn get_in_flight_snapshot (& self) -> HashMap < i64 , usize >
  ```

### `src/api/usage.rs` (Role: api, Lines: 48)
- **Responsibility**: Core api logic in src/api/usage.rs
- **Imports**: use crate :: api :: state :: AppState , use crate :: domain :: usage :: UsageFilter , use crate :: infra :: usage_db :: { get_filtered_daily_breakdown , get_filtered_model_breakdown , get_filtered_summaries , } , use axum :: { extract :: { Query , State } , response :: IntoResponse , Json , } , use serde :: Deserialize , use serde_json :: json 
- **Types & Enums**:
  ```rust
  pub struct UsageQuery
  ```
- **Public Functions & Signatures**:
  ```rust
  async fn get_usage_metrics (State (state) : State < AppState > , Query (query) : Query < UsageQuery > ,) -> impl IntoResponse
  ```

### `src/api.rs` (Role: api, Lines: 62)
- **Responsibility**: Core api logic in src/api.rs
- **Imports**: use crate :: api :: anthropic :: anthropic_messages , use crate :: api :: chat :: chat_completions , use crate :: api :: dashboard :: { add_token , delete_token , logout , show_dashboard , show_login , submit_login , } , use crate :: api :: files :: { upload_file_anthropic , upload_file_openai } , use crate :: api :: health :: { health , root } , use crate :: api :: middleware :: require_api_key , use crate :: api :: models :: list_models , use crate :: api :: state :: AppState , use crate :: api :: usage :: get_usage_metrics , use axum :: { middleware :: from_fn_with_state , routing :: { get , post } , Router , } , use tower_http :: cors :: CorsLayer , use tower_http :: services :: ServeDir 
- **Public Functions & Signatures**:
  ```rust
  fn build_router (state : AppState) -> Router
  ```

### `src/cli/diagnostic.rs` (Role: cli, Lines: 116)
- **Responsibility**: Core cli logic in src/cli/diagnostic.rs
- **Imports**: use crate :: domain :: upstream :: PowChallenge , use crate :: infra :: db :: { get_tokens , init_db , open_db } , use crate :: infra :: pow :: PowSolver , use anyhow :: { Context , Result } , use std :: time :: Instant 
- **Public Functions & Signatures**:
  ```rust
  async fn run_diagnostics (db_path : & str , wasm_path : & str , server_url : Option < & str > ,) -> Result < () >
  ```

### `src/cli/service.rs` (Role: cli, Lines: 104)
- **Responsibility**: Core cli logic in src/cli/service.rs
- **Imports**: use anyhow :: { Context , Result } , use std :: fs :: { create_dir_all , remove_file , write } , use std :: path :: PathBuf , use std :: process :: Command 
- **Public Functions & Signatures**:
  ```rust
  fn install_user_service (custom_bin : Option < & str >) -> Result < () >
  fn uninstall_user_service () -> Result < () >
  fn service_status () -> Result < () >
  ```

### `src/cli/token_cmd.rs` (Role: cli, Lines: 138)
- **Responsibility**: Core cli logic in src/cli/token_cmd.rs
- **Imports**: use crate :: domain :: token :: Token , use crate :: infra :: db :: { add_token as db_add , delete_token , get_tokens , init_db , open_db } , use crate :: infra :: deepseek_client :: DeepSeekClient , use anyhow :: { Context , Result } , use std :: time :: Instant 
- **Public Functions & Signatures**:
  ```rust
  async fn list_tokens (db_path : & str) -> Result < () >
  async fn add_token (token : & str , alias : Option < & str > , db_path : & str) -> Result < () >
  async fn remove_token (token_id : i64 , db_path : & str) -> Result < () >
  async fn test_tokens (target_id : Option < i64 > , db_path : & str) -> Result < () >
  ```

### `src/cli/usage_cmd.rs` (Role: cli, Lines: 179)
- **Responsibility**: Core cli logic in src/cli/usage_cmd.rs
- **Imports**: use crate :: domain :: usage :: { format_metric , UsageFilter } , use crate :: infra :: db :: { get_tokens , init_db , open_db } , use crate :: infra :: usage_db :: { get_filtered_daily_breakdown , get_filtered_model_breakdown , get_filtered_summaries , } , use anyhow :: { Context , Result } , use serde_json :: json 
- **Types & Enums**:
  ```rust
  pub struct UsageViewArgs
  ```
- **Public Functions & Signatures**:
  ```rust
  async fn display_usage (args : UsageViewArgs) -> Result < () >
  ```

### `src/cli.rs` (Role: cli, Lines: 155)
- **Responsibility**: Core cli logic in src/cli.rs
- **Imports**: use clap :: { Args , Parser , Subcommand } 
- **Types & Enums**:
  ```rust
  pub struct Cli
  pub enum Commands
  pub struct ServeArgs
  pub struct StatusArgs
  pub struct TokenArgs
  pub enum TokenSubcommands
  pub struct TestArgs
  pub struct ServiceArgs
  pub enum ServiceSubcommands
  pub struct UsageArgs
  ```

### `src/config.rs` (Role: general, Lines: 57)
- **Responsibility**: Core general logic in src/config.rs
- **Imports**: use std :: env 
- **Types & Enums**:
  ```rust
  pub struct AppConfig
  ```
- **Public Functions & Signatures**:
  ```rust
  fn from_env () -> Self
  ```

### `src/domain/anthropic.rs` (Role: domain, Lines: 63)
- **Responsibility**: Core domain logic in src/domain/anthropic.rs
- **Imports**: use serde :: { Deserialize , Serialize } 
- **Types & Enums**:
  ```rust
  pub struct AnthropicMessageRequest
  pub struct AnthropicMessage
  pub enum AnthropicContent
  pub struct AnthropicBlock
  pub struct AnthropicImageSource
  pub struct AnthropicMessageResponse
  pub struct AnthropicUsage
  ```

### `src/domain/openai.rs` (Role: domain, Lines: 365)
- **Responsibility**: Core domain logic in src/domain/openai.rs
- **Imports**: use serde :: { Deserialize , Serialize } 
- **Types & Enums**:
  ```rust
  pub enum MessageContent
  pub struct ContentPart
  pub struct ImageUrl
  pub struct FileReference
  pub struct ToolCall
  pub struct FunctionCall
  pub struct ChatMessage
  pub struct ChatCompletionRequest
  pub struct ChatCompletionResponse
  pub struct ChatChoice
  pub struct ResponseMessage
  pub struct ChatCompletionChunk
  pub struct ChunkChoice
  pub struct ChunkDelta
  pub struct Usage
  pub struct ModelObject
  pub struct ModelList
  ```
- **Public Functions & Signatures**:
  ```rust
  fn as_text (& self) -> String
  fn text (t : impl Into < String >) -> Self
  fn image_url (url : impl Into < String >) -> Self
  fn new (role : impl Into < String > , content : impl Into < String >) -> Self
  fn user (content : impl Into < String >) -> Self
  fn assistant (content : impl Into < String >) -> Self
  fn system (content : impl Into < String >) -> Self
  fn text_content (& self) -> String
  fn is_reasoning_requested (& self) -> bool
  fn is_search_requested (& self) -> bool
  ```

### `src/domain/session.rs` (Role: domain, Lines: 92)
- **Responsibility**: Core domain logic in src/domain/session.rs
- **Imports**: use crate :: domain :: openai :: { ChatMessage , ToolCall } , use serde :: { Deserialize , Serialize } , use sha2 :: { Digest , Sha256 } 
- **Types & Enums**:
  ```rust
  pub struct Session
  ```
- **Public Functions & Signatures**:
  ```rust
  fn new (token_id : i64 , session_id : String , parent_message_id : i64 , last_used : f64) -> Self
  fn next_parent_id (current_parent_id : i64) -> i64
  fn compute_signature (messages : & [ChatMessage] , model : & str , scope : & str) -> String
  fn compute_next_signature (messages : & [ChatMessage] , model : & str , assistant_content : & str , tool_calls : Option < & [ToolCall] > ,) -> String
  ```

### `src/domain/token.rs` (Role: domain, Lines: 39)
- **Responsibility**: Core domain logic in src/domain/token.rs
- **Imports**: use serde :: { Deserialize , Serialize } 
- **Types & Enums**:
  ```rust
  pub struct Token
  ```
- **Public Functions & Signatures**:
  ```rust
  fn is_active (& self) -> bool
  fn is_rate_limited (& self) -> bool
  fn is_expired_rate_limit (& self , now : f64) -> bool
  fn masked_token (& self) -> String
  ```

### `src/domain/upstream.rs` (Role: domain, Lines: 67)
- **Responsibility**: Core domain logic in src/domain/upstream.rs
- **Imports**: use serde :: { Deserialize , Serialize } 
- **Types & Enums**:
  ```rust
  pub struct PowChallenge
  pub struct PowChallengeWrapper
  pub struct PowChallengeBizData
  pub struct PowChallengeInner
  pub struct PowSolution
  pub struct CreateChatResponse
  pub struct CreateChatBizData
  pub struct CreateChatInner
  pub struct ChatSessionObject
  pub struct FileRecord
  ```

### `src/domain/usage.rs` (Role: domain, Lines: 63)
- **Responsibility**: Core domain logic in src/domain/usage.rs
- **Imports**: use serde :: { Deserialize , Serialize } 
- **Types & Enums**:
  ```rust
  pub struct UsageRecord
  pub struct UsageSummary
  pub struct DailyUsage
  pub struct ModelUsage
  pub struct UsageFilter
  ```
- **Public Functions & Signatures**:
  ```rust
  fn format_metric (val : u64 , raw : bool) -> String
  ```

### `src/domain.rs` (Role: domain, Lines: 6)
- **Responsibility**: Core domain logic in src/domain.rs

### `src/infra/assets.rs` (Role: infra, Lines: 85)
- **Responsibility**: Core infra logic in src/infra/assets.rs
- **Imports**: use std :: path :: { Path , PathBuf } 
- **Public Functions & Signatures**:
  ```rust
  fn resolve_asset_dir (relative : & str) -> PathBuf
  fn resolve_templates_pattern () -> String
  fn resolve_wasm_path (default_rel : & str) -> String
  fn resolve_db_path (custom : Option < & str >) -> String
  ```

### `src/infra/db.rs` (Role: infra, Lines: 356)
- **Responsibility**: Core infra logic in src/infra/db.rs
- **Imports**: use crate :: domain :: session :: Session , use crate :: domain :: token :: Token , use anyhow :: { Context , Result } , use rusqlite :: params , use std :: collections :: HashMap , use std :: time :: { SystemTime , UNIX_EPOCH } , use tokio_rusqlite :: Connection 
- **Public Functions & Signatures**:
  ```rust
  fn now_timestamp () -> f64
  async fn open_db (path : & str) -> Result < Connection >
  async fn init_db (conn : & Connection) -> Result < () >
  async fn add_token (conn : & Connection , token : & str , alias : Option < & str >) -> Result < () >
  async fn get_tokens (conn : & Connection) -> Result < Vec < Token > >
  async fn get_token (conn : & Connection , token_id : i64) -> Result < Option < Token > >
  async fn delete_token (conn : & Connection , token_id : i64) -> Result < () >
  async fn mark_limited (conn : & Connection , token_id : i64 , cooldown_secs : u64) -> Result < () >
  async fn mark_active (conn : & Connection , token_id : i64) -> Result < () >
  async fn touch_token (conn : & Connection , token_id : i64) -> Result < () >
  async fn pick_token (conn : & Connection , exclude : & [i64] , in_flight : & HashMap < i64 , usize > , concurrency_cap : usize ,) -> Result < Option < Token > >
  async fn find_session (conn : & Connection , signature : & str) -> Result < Option < Session > >
  async fn save_session (conn : & Connection , signature : & str , session : & Session) -> Result < () >
  async fn delete_sessions_for_chat (conn : & Connection , token_id : i64 , session_id : & str ,) -> Result < () >
  async fn record_file (conn : & Connection , file_id : & str , token_id : i64) -> Result < () >
  async fn get_file_token (conn : & Connection , file_id : & str) -> Result < Option < i64 > >
  ```

### `src/infra/deepseek_client.rs` (Role: infra, Lines: 254)
- **Responsibility**: Core infra logic in src/infra/deepseek_client.rs
- **Imports**: use crate :: domain :: upstream :: { CreateChatResponse , PowChallenge , PowChallengeWrapper } , use anyhow :: { anyhow , Context , Result } , use reqwest :: header :: { HeaderMap , HeaderValue , AUTHORIZATION , CONTENT_TYPE , USER_AGENT } , use reqwest :: Client , use serde_json :: json 
- **Types & Enums**:
  ```rust
  pub struct DeepSeekClient
  pub struct CompletionArgs
  ```
- **Public Functions & Signatures**:
  ```rust
  fn new () -> Self
  fn build_headers (& self , token : & str , pow : Option < & str >) -> Result < HeaderMap >
  async fn create_pow_challenge (& self , token : & str , target_path : & str ,) -> Result < PowChallenge >
  async fn create_chat_session (& self , token : & str) -> Result < String >
  async fn send_completion_request (& self , args : CompletionArgs) -> Result < reqwest :: Response >
  async fn upload_file (& self , token : & str , pow_resp : & str , filename : & str , content_type : & str , bytes : Vec < u8 > ,) -> Result < String >
  async fn download_file (& self , token : & str , file_id : & str) -> Result < Vec < u8 > >
  async fn fetch_bytes (& self , url : & str) -> Result < Vec < u8 > >
  ```

### `src/infra/dsml.rs` (Role: infra, Lines: 365)
- **Responsibility**: Core infra logic in src/infra/dsml.rs
- **Imports**: use crate :: domain :: openai :: { FunctionCall , ToolCall } , use once_cell :: sync :: Lazy , use regex :: Regex , use serde_json :: Value , use uuid :: Uuid 
- **Types & Enums**:
  ```rust
  pub struct ParsedDsml
  ```
- **Public Functions & Signatures**:
  ```rust
  fn find_dsml_block_start (text : & str) -> Option < usize >
  fn safe_unambiguous_len (text : & str) -> usize
  fn parse_dsml (text : & str) -> ParsedDsml
  ```

### `src/infra/media.rs` (Role: infra, Lines: 208)
- **Responsibility**: Core infra logic in src/infra/media.rs
- **Imports**: use crate :: domain :: openai :: { ChatMessage , MessageContent } , use crate :: infra :: db :: record_file , use crate :: infra :: deepseek_client :: DeepSeekClient , use crate :: infra :: pow :: PowSolver , use crate :: infra :: rehome :: rehome_foreign_files , use anyhow :: { anyhow , Context , Result } , use base64 :: { engine :: general_purpose :: STANDARD as B64 , Engine as _ } , use std :: sync :: Arc , use tokio_rusqlite :: Connection 
- **Types & Enums**:
  ```rust
  pub struct MediaContext
  pub struct DecodedImage
  ```
- **Public Functions & Signatures**:
  ```rust
  async fn resolve_message_media (ctx : & MediaContext < '_ > , messages : & [ChatMessage] ,) -> Result < Vec < String > >
  async fn parse_image_source (client : & DeepSeekClient , url : & str) -> Result < DecodedImage >
  fn detect_image_format (bytes : & [u8]) -> (& 'static str , & 'static str)
  ```

### `src/infra/pow.rs` (Role: infra, Lines: 95)
- **Responsibility**: Core infra logic in src/infra/pow.rs
- **Imports**: use crate :: domain :: upstream :: { PowChallenge , PowSolution } , use anyhow :: { anyhow , Context , Result } , use base64 :: { engine :: general_purpose :: STANDARD as B64 , Engine as _ } , use std :: sync :: Arc , use wasmtime :: { Engine , Instance , Module , Store } 
- **Types & Enums**:
  ```rust
  pub struct PowSolver
  ```
- **Public Functions & Signatures**:
  ```rust
  fn new (wasm_path : & str) -> Result < Self >
  fn solve (& self , challenge : & PowChallenge , target_path : & str) -> Result < String >
  ```

### `src/infra/prompt.rs` (Role: infra, Lines: 176)
- **Responsibility**: Core infra logic in src/infra/prompt.rs
- **Imports**: use crate :: domain :: openai :: ChatMessage , use serde_json :: Value 
- **Public Functions & Signatures**:
  ```rust
  fn build_prompt_for_turn (messages : & [ChatMessage] , tools : Option < & [Value] > , is_first : bool ,) -> String
  fn format_tools_section (tools : & [Value]) -> Option < String >
  ```

### `src/infra/rehome.rs` (Role: infra, Lines: 94)
- **Responsibility**: Core infra logic in src/infra/rehome.rs
- **Imports**: use crate :: infra :: db :: { get_file_token , get_token , record_file } , use crate :: infra :: deepseek_client :: DeepSeekClient , use crate :: infra :: pow :: PowSolver , use anyhow :: { Context , Result } , use std :: sync :: Arc , use tokio_rusqlite :: Connection 
- **Public Functions & Signatures**:
  ```rust
  async fn rehome_foreign_files (db : & Connection , client : & DeepSeekClient , solver : & Arc < PowSolver > , file_ids : & [String] , target_token_id : i64 , target_token : & str ,) -> Result < Vec < String > >
  ```

### `src/infra/sse.rs` (Role: infra, Lines: 175)
- **Responsibility**: Core infra logic in src/infra/sse.rs
- **Types & Enums**:
  ```rust
  pub struct ExtractedChunk
  pub enum SseLineResult
  ```
- **Public Functions & Signatures**:
  ```rust
  fn drain_sse_lines (buffer : & mut String , bytes : & [u8]) -> Vec < String >
  fn parse_sse_line (line : & str , think_open : & mut bool) -> SseLineResult
  fn extract_chunks_from_event (val : & serde_json :: Value , think_open : & mut bool ,) -> Vec < ExtractedChunk >
  ```

### `src/infra/usage_db.rs` (Role: infra, Lines: 216)
- **Responsibility**: Core infra logic in src/infra/usage_db.rs
- **Imports**: use crate :: domain :: usage :: { DailyUsage , ModelUsage , UsageFilter , UsageSummary } , use anyhow :: { Context , Result } , use rusqlite :: params , use tokio_rusqlite :: Connection 
- **Public Functions & Signatures**:
  ```rust
  async fn init_usage_table (conn : & Connection) -> Result < () >
  async fn record_usage (conn : & Connection , model : & str , prompt_tokens : u32 , completion_tokens : u32 , token_id : Option < i64 > ,) -> Result < () >
  async fn get_all_summaries (conn : & Connection) -> Result < Vec < UsageSummary > >
  async fn get_filtered_summaries (conn : & Connection , filter : & UsageFilter ,) -> Result < Vec < UsageSummary > >
  async fn get_daily_breakdown (conn : & Connection , limit : usize) -> Result < Vec < DailyUsage > >
  async fn get_filtered_daily_breakdown (conn : & Connection , limit : usize , filter : & UsageFilter ,) -> Result < Vec < DailyUsage > >
  async fn get_model_breakdown (conn : & Connection) -> Result < Vec < ModelUsage > >
  async fn get_filtered_model_breakdown (conn : & Connection , filter : & UsageFilter ,) -> Result < Vec < ModelUsage > >
  ```

### `src/infra/watchdog.rs` (Role: infra, Lines: 60)
- **Responsibility**: Core infra logic in src/infra/watchdog.rs
- **Imports**: use crate :: infra :: db :: { get_tokens , mark_active , now_timestamp } , use anyhow :: Result , use std :: time :: Duration , use tokio_rusqlite :: Connection , use tracing :: info 
- **Public Functions & Signatures**:
  ```rust
  async fn check_and_recover_tokens (conn : & Connection) -> Result < usize >
  fn start_token_watchdog (conn : Connection , interval : Duration) -> tokio :: task :: JoinHandle < () >
  ```

### `src/infra.rs` (Role: infra, Lines: 11)
- **Responsibility**: Core infra logic in src/infra.rs

### `src/lib.rs` (Role: general, Lines: 6)
- **Responsibility**: Core general logic in src/lib.rs

### `src/main.rs` (Role: general, Lines: 150)
- **Responsibility**: Core general logic in src/main.rs
- **Imports**: use anyhow :: { Context , Result } , use clap :: Parser , use deeperseeker :: api :: build_router , use deeperseeker :: api :: state :: AppState , use deeperseeker :: cli :: diagnostic :: run_diagnostics , use deeperseeker :: cli :: service :: { install_user_service , service_status , uninstall_user_service } , use deeperseeker :: cli :: token_cmd :: { add_token , list_tokens , remove_token , test_tokens } , use deeperseeker :: cli :: usage_cmd :: display_usage , use deeperseeker :: cli :: { Cli , Commands , ServeArgs , ServiceArgs , ServiceSubcommands , TokenArgs , TokenSubcommands , } , use deeperseeker :: config :: AppConfig , use deeperseeker :: infra :: db :: { init_db , open_db } , use deeperseeker :: infra :: deepseek_client :: DeepSeekClient , use deeperseeker :: infra :: pow :: PowSolver , use deeperseeker :: tui :: run_status , use std :: collections :: HashMap , use std :: sync :: Arc , use tera :: Tera , use tokio :: net :: TcpListener , use tokio :: sync :: Mutex , use tracing :: info 

### `src/tui/tabs.rs` (Role: tui, Lines: 264)
- **Responsibility**: Core tui logic in src/tui/tabs.rs
- **Imports**: use crate :: domain :: token :: Token , use crate :: domain :: usage :: { format_metric , UsageSummary } , use crate :: tui :: views :: RenderState , use ratatui :: { layout :: { Constraint , Direction , Layout , Rect } , style :: { Color , Modifier , Style } , text :: { Line , Span } , widgets :: { Block , BorderType , Borders , Cell , Gauge , Paragraph , Row , Table , Wrap } , Frame , } 
- **Public Functions & Signatures**:
  ```rust
  fn render_monitor_tab (f : & mut Frame , area : Rect , state : & RenderState)
  fn render_usage_tab (f : & mut Frame , area : Rect , summaries : & [UsageSummary])
  fn render_tokens_tab (f : & mut Frame , area : Rect , state : & RenderState)
  fn render_tokens_table (f : & mut Frame , area : Rect , tokens : & [Token])
  fn render_diagnostics_tab (f : & mut Frame , area : Rect , state : & RenderState)
  ```

### `src/tui/views.rs` (Role: tui, Lines: 198)
- **Responsibility**: Core tui logic in src/tui/views.rs
- **Imports**: use crate :: domain :: token :: Token , use crate :: domain :: usage :: UsageSummary , use crate :: tui :: tabs :: { render_diagnostics_tab , render_monitor_tab , render_tokens_tab , render_usage_tab , } , use ratatui :: { layout :: { Alignment , Constraint , Direction , Layout , Rect } , style :: { Color , Modifier , Style } , text :: { Line , Span } , widgets :: { Block , BorderType , Borders , Paragraph } , Frame , } 
- **Types & Enums**:
  ```rust
  pub enum ActiveTab
  pub struct RenderState
  ```
- **Public Functions & Signatures**:
  ```rust
  fn next (self) -> Self
  fn render_ui (f : & mut Frame , state : & RenderState)
  ```

### `src/tui.rs` (Role: tui, Lines: 248)
- **Responsibility**: Core tui logic in src/tui.rs
- **Imports**: use crate :: domain :: token :: Token , use crate :: domain :: usage :: { format_metric , UsageSummary } , use crate :: infra :: db :: { get_tokens , open_db } , use crate :: infra :: pow :: PowSolver , use crate :: infra :: usage_db :: get_all_summaries , use crate :: tui :: views :: { render_ui , ActiveTab , RenderState } , use anyhow :: { Context , Result } , use crossterm :: { event :: { self , Event , KeyCode } , execute , terminal :: { disable_raw_mode , enable_raw_mode , EnterAlternateScreen , LeaveAlternateScreen } , } , use ratatui :: { backend :: CrosstermBackend , Terminal } , use serde_json :: Value , use std :: io :: { stdout , IsTerminal } , use std :: time :: { Duration , Instant } 
- **Types & Enums**:
  ```rust
  pub struct TuiData
  ```
- **Public Functions & Signatures**:
  ```rust
  async fn run_status (server_url : & str , db_path : & str , plain : bool) -> Result < () >
  async fn render_plain_status (server_url : & str , db_path : & str) -> Result < () >
  ```

## 4. Execution Lifecycle Trace
1. **Startup**: Entrypoint parses CLI flags & dispatches command.
2. **Execution**: Core domain logic processes inputs and evaluates rules.
3. **Persistence / I/O**: Domain logic calls infra for disk/terminal I/O.
4. **Exit**: Graceful termination with standard exit codes.

## 5. Verification Commands
```bash
cargo build --release --target x86_64-unknown-linux-gnu
cargo test --all-targets
cargo clippy --all-targets -- -D warnings && cargo fmt --check
```
