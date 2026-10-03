# CODEBASE.md: deeperseeker Semantic Digest

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

### `src/api/anthropic.rs` (Role: api, Lines: 122)
- **Responsibility**: Core api logic in src/api/anthropic.rs
- **Imports**: use crate :: api :: chat :: chat_completions , use crate :: api :: state :: AppState , use crate :: domain :: anthropic :: { AnthropicBlock , AnthropicContent , AnthropicMessage , AnthropicMessageRequest , AnthropicMessageResponse , AnthropicUsage , } , use crate :: domain :: openai :: { ChatCompletionRequest , ChatCompletionResponse , ChatMessage , MessageContent , } , use axum :: { extract :: State , http :: StatusCode , response :: { IntoResponse , Response } , Json , } , use serde_json :: json 
- **Public Functions & Signatures**:
  ```rust
  async fn anthropic_messages (State (state) : State < AppState > , Json (req) : Json < AnthropicMessageRequest > ,) -> Result < Response , (StatusCode , Json < serde_json :: Value >) >
  ```

### `src/api/chat.rs` (Role: api, Lines: 259)
- **Responsibility**: Core api logic in src/api/chat.rs
- **Imports**: use crate :: api :: chat_stream :: { handle_streaming_response , handle_unary_response } , use crate :: api :: state :: AppState , use crate :: domain :: openai :: ChatCompletionRequest , use crate :: domain :: session :: compute_signature , use crate :: domain :: token :: Token , use crate :: infra :: db :: { find_session , mark_active , mark_limited , pick_token , touch_token } , use crate :: infra :: deepseek_client :: CompletionArgs , use crate :: infra :: prompt :: build_prompt_for_turn , use crate :: infra :: rehome :: rehome_foreign_files , use axum :: { extract :: State , http :: StatusCode , response :: Response , Json } , use serde_json :: json 
- **Public Functions & Signatures**:
  ```rust
  async fn chat_completions (State (state) : State < AppState > , Json (req) : Json < ChatCompletionRequest > ,) -> Result < Response , (StatusCode , Json < serde_json :: Value >) >
  ```

### `src/api/chat_stream.rs` (Role: api, Lines: 369)
- **Responsibility**: Core api logic in src/api/chat_stream.rs
- **Imports**: use crate :: api :: state :: AppState , use crate :: domain :: openai :: { ChatChoice , ChatCompletionChunk , ChatCompletionResponse , ChatMessage , ChunkChoice , ChunkDelta , ResponseMessage , Usage , } , use crate :: domain :: session :: { compute_next_signature , next_parent_id , Session } , use crate :: infra :: db :: save_session , use axum :: { body :: Body , http :: { header :: CONTENT_TYPE , StatusCode } , response :: { IntoResponse , Response } , Json , } , use futures :: StreamExt , use std :: time :: { SystemTime , UNIX_EPOCH } , use uuid :: Uuid 
- **Types & Enums**:
  ```rust
  pub struct ExtractedChunk
  pub enum SseLineResult
  ```
- **Public Functions & Signatures**:
  ```rust
  async fn handle_streaming_response (state : & AppState , model : String , token_id : i64 , session_id : String , parent_id : i64 , req_messages : Vec < ChatMessage > , upstream_resp : reqwest :: Response ,) -> Result < Response , (StatusCode , String) >
  async fn handle_unary_response (state : & AppState , model : String , token_id : i64 , session_id : String , parent_id : i64 , req_messages : & [ChatMessage] , upstream_resp : reqwest :: Response ,) -> Result < Response , (StatusCode , String) >
  fn extract_chunks_from_event (val : & serde_json :: Value , think_open : & mut bool ,) -> Vec < ExtractedChunk >
  ```

### `src/api/dashboard.rs` (Role: api, Lines: 147)
- **Responsibility**: Core api logic in src/api/dashboard.rs
- **Imports**: use crate :: api :: state :: AppState , use crate :: infra :: db :: { add_token as db_add_token , delete_token as db_delete_token , get_tokens } , use axum :: { extract :: { Form , Path , State } , http :: { header :: { COOKIE , SET_COOKIE } , HeaderMap , StatusCode } , response :: { Html , IntoResponse , Redirect , Response } , } , use serde :: { Deserialize , Serialize } , use sha2 :: { Digest , Sha256 } , use tera :: Context 
- **Types & Enums**:
  ```rust
  pub struct LoginForm
  pub struct AddTokenForm
  pub struct DashboardTokenView
  ```
- **Public Functions & Signatures**:
  ```rust
  async fn show_login (State (state) : State < AppState >) -> Response
  async fn submit_login (State (state) : State < AppState > , Form (form) : Form < LoginForm > ,) -> Response
  async fn logout () -> Response
  async fn show_dashboard (State (state) : State < AppState > , headers : HeaderMap ,) -> Response
  async fn add_token (State (state) : State < AppState > , headers : HeaderMap , Form (form) : Form < AddTokenForm > ,) -> Response
  async fn delete_token (State (state) : State < AppState > , headers : HeaderMap , Path (token_id) : Path < i64 > ,) -> Response
  ```

### `src/api/files.rs` (Role: api, Lines: 130)
- **Responsibility**: Core api logic in src/api/files.rs
- **Imports**: use crate :: api :: state :: AppState , use crate :: infra :: db :: { pick_token , record_file } , use axum :: { extract :: { Multipart , State } , http :: StatusCode , Json , } , use serde_json :: json , use std :: time :: { SystemTime , UNIX_EPOCH } 
- **Public Functions & Signatures**:
  ```rust
  async fn upload_file_openai (State (state) : State < AppState > , mut multipart : Multipart ,) -> Result < Json < serde_json :: Value > , (StatusCode , Json < serde_json :: Value >) >
  async fn upload_file_anthropic (State (state) : State < AppState > , multipart : Multipart ,) -> Result < Json < serde_json :: Value > , (StatusCode , Json < serde_json :: Value >) >
  ```

### `src/api/health.rs` (Role: api, Lines: 13)
- **Responsibility**: Core api logic in src/api/health.rs
- **Imports**: use axum :: { response :: { IntoResponse , Redirect } , Json , } , use serde_json :: json 
- **Public Functions & Signatures**:
  ```rust
  async fn root () -> impl IntoResponse
  async fn health () -> impl IntoResponse
  ```

### `src/api/middleware.rs` (Role: api, Lines: 51)
- **Responsibility**: Core api logic in src/api/middleware.rs
- **Imports**: use crate :: api :: state :: AppState , use axum :: { extract :: { Request , State } , http :: { header :: AUTHORIZATION , StatusCode } , middleware :: Next , response :: { IntoResponse , Response } , Json , } , use serde_json :: json 
- **Public Functions & Signatures**:
  ```rust
  async fn require_api_key (State (state) : State < AppState > , req : Request , next : Next ,) -> Response
  ```

### `src/api/models.rs` (Role: api, Lines: 24)
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

### `src/api.rs` (Role: api, Lines: 54)
- **Responsibility**: Core api logic in src/api.rs
- **Imports**: use crate :: api :: anthropic :: anthropic_messages , use crate :: api :: chat :: chat_completions , use crate :: api :: dashboard :: { add_token , delete_token , logout , show_dashboard , show_login , submit_login , } , use crate :: api :: files :: { upload_file_anthropic , upload_file_openai } , use crate :: api :: health :: { health , root } , use crate :: api :: middleware :: require_api_key , use crate :: api :: models :: list_models , use crate :: api :: state :: AppState , use axum :: { middleware :: from_fn_with_state , routing :: { get , post } , Router , } , use tower_http :: cors :: CorsLayer , use tower_http :: services :: ServeDir 
- **Public Functions & Signatures**:
  ```rust
  fn build_router (state : AppState) -> Router
  ```

### `src/config.rs` (Role: general, Lines: 55)
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

### `src/domain/anthropic.rs` (Role: domain, Lines: 52)
- **Responsibility**: Core domain logic in src/domain/anthropic.rs
- **Imports**: use serde :: { Deserialize , Serialize } 
- **Types & Enums**:
  ```rust
  pub struct AnthropicMessageRequest
  pub struct AnthropicMessage
  pub enum AnthropicContent
  pub struct AnthropicBlock
  pub struct AnthropicMessageResponse
  pub struct AnthropicUsage
  ```

### `src/domain/openai.rs` (Role: domain, Lines: 143)
- **Responsibility**: Core domain logic in src/domain/openai.rs
- **Imports**: use serde :: { Deserialize , Serialize } 
- **Types & Enums**:
  ```rust
  pub enum MessageContent
  pub struct ContentPart
  pub struct ImageUrl
  pub struct FileReference
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
  ```

### `src/domain/session.rs` (Role: domain, Lines: 64)
- **Responsibility**: Core domain logic in src/domain/session.rs
- **Imports**: use serde :: { Deserialize , Serialize } , use sha2 :: { Digest , Sha256 } 
- **Types & Enums**:
  ```rust
  pub struct Session
  ```
- **Public Functions & Signatures**:
  ```rust
  fn new (token_id : i64 , session_id : String , parent_message_id : i64 , last_used : f64) -> Self
  fn next_parent_id (current_parent_id : i64) -> i64
  fn compute_signature (messages : & [crate :: domain :: openai :: ChatMessage] , model : & str , scope : & str ,) -> String
  fn compute_next_signature (messages : & [crate :: domain :: openai :: ChatMessage] , model : & str , assistant_content : & str ,) -> String
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

### `src/domain.rs` (Role: domain, Lines: 5)
- **Responsibility**: Core domain logic in src/domain.rs

### `src/infra/db.rs` (Role: infra, Lines: 336)
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
  async fn save_session (conn : & Connection , signature : & str , session : & Session ,) -> Result < () >
  async fn delete_sessions_for_chat (conn : & Connection , token_id : i64 , session_id : & str ,) -> Result < () >
  async fn record_file (conn : & Connection , file_id : & str , token_id : i64) -> Result < () >
  async fn get_file_token (conn : & Connection , file_id : & str) -> Result < Option < i64 > >
  ```

### `src/infra/deepseek_client.rs` (Role: infra, Lines: 245)
- **Responsibility**: Core infra logic in src/infra/deepseek_client.rs
- **Imports**: use crate :: domain :: upstream :: { CreateChatResponse , PowChallenge , PowChallengeWrapper , } , use anyhow :: { anyhow , Context , Result } , use reqwest :: header :: { HeaderMap , HeaderValue , AUTHORIZATION , CONTENT_TYPE , USER_AGENT } , use reqwest :: Client , use serde_json :: json 
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
  async fn send_completion_request (& self , args : CompletionArgs ,) -> Result < reqwest :: Response >
  async fn upload_file (& self , token : & str , pow_resp : & str , filename : & str , content_type : & str , bytes : Vec < u8 > ,) -> Result < String >
  async fn download_file (& self , token : & str , file_id : & str) -> Result < Vec < u8 > >
  ```

### `src/infra/pow.rs` (Role: infra, Lines: 94)
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

### `src/infra/prompt.rs` (Role: infra, Lines: 86)
- **Responsibility**: Core infra logic in src/infra/prompt.rs
- **Imports**: use crate :: domain :: openai :: ChatMessage 
- **Public Functions & Signatures**:
  ```rust
  fn build_prompt_for_turn (messages : & [ChatMessage] , is_first : bool) -> String
  ```

### `src/infra/rehome.rs` (Role: infra, Lines: 94)
- **Responsibility**: Core infra logic in src/infra/rehome.rs
- **Imports**: use crate :: infra :: db :: { get_file_token , get_token , record_file } , use crate :: infra :: deepseek_client :: DeepSeekClient , use crate :: infra :: pow :: PowSolver , use anyhow :: { Context , Result } , use std :: sync :: Arc , use tokio_rusqlite :: Connection 
- **Public Functions & Signatures**:
  ```rust
  async fn rehome_foreign_files (db : & Connection , client : & DeepSeekClient , solver : & Arc < PowSolver > , file_ids : & [String] , target_token_id : i64 , target_token : & str ,) -> Result < Vec < String > >
  ```

### `src/infra.rs` (Role: infra, Lines: 5)
- **Responsibility**: Core infra logic in src/infra.rs

### `src/lib.rs` (Role: general, Lines: 4)
- **Responsibility**: Core general logic in src/lib.rs

### `src/main.rs` (Role: general, Lines: 60)
- **Responsibility**: Core general logic in src/main.rs
- **Imports**: use deeperseeker :: api :: build_router , use deeperseeker :: api :: state :: AppState , use deeperseeker :: config :: AppConfig , use deeperseeker :: infra :: db :: { init_db , open_db } , use deeperseeker :: infra :: deepseek_client :: DeepSeekClient , use deeperseeker :: infra :: pow :: PowSolver , use anyhow :: { Context , Result } , use std :: collections :: HashMap , use std :: sync :: Arc , use tera :: Tera , use tokio :: net :: TcpListener , use tokio :: sync :: Mutex , use tracing :: info 

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
