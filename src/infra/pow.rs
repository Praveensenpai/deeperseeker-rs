use crate::domain::upstream::{PowChallenge, PowSolution};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use std::sync::Arc;
use wasmtime::{Engine, Instance, Module, Store};

#[derive(Clone)]
pub struct PowSolver {
    engine: Engine,
    module: Arc<Module>,
}

impl PowSolver {
    pub fn new(wasm_path: &str) -> Result<Self> {
        let engine = Engine::default();
        let module = Module::from_file(&engine, wasm_path)
            .with_context(|| format!("Failed to load WASM file from {wasm_path}"))?;
        Ok(Self {
            engine,
            module: Arc::new(module),
        })
    }

    pub fn solve(&self, challenge: &PowChallenge, target_path: &str) -> Result<String> {
        let answer = self.find_answer(challenge)?;
        let solution = PowSolution {
            algorithm: challenge
                .algorithm
                .clone()
                .unwrap_or_else(|| "DeepSeekHashV1".to_string()),
            challenge: challenge.challenge.clone(),
            salt: challenge.salt.clone(),
            answer,
            signature: challenge.signature.clone(),
            target_path: target_path.to_string(),
        };

        let json_bytes = serde_json::to_vec(&solution)?;
        Ok(B64.encode(json_bytes))
    }

    fn find_answer(&self, ch: &PowChallenge) -> Result<u64> {
        let mut store = Store::new(&self.engine, ());
        let instance = Instance::new(&mut store, &self.module, &[])
            .context("Failed to instantiate PoW WASM module")?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow!("WASM missing memory export"))?;

        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "alloc")
            .context("Failed to get alloc func")?;

        let (ch_ptr, ch_len) = write_bytes(&mut store, &instance, &alloc, memory, &ch.challenge)?;
        let (salt_ptr, salt_len) = write_bytes(&mut store, &instance, &alloc, memory, &ch.salt)?;

        let solve = instance
            .get_typed_func::<(i32, i32, i32, i32, i64, i64), i64>(&mut store, "solve_pow")
            .context("Failed to get solve_pow func")?;

        let res = solve.call(
            &mut store,
            (
                ch_ptr,
                ch_len,
                salt_ptr,
                salt_len,
                ch.expire_at,
                ch.difficulty as i64,
            ),
        )?;

        let unsigned_res = res as u64;
        if unsigned_res == 0xFFFF_FFFF_FFFF_FFFF {
            return Err(anyhow!("PoW difficulty unsolved"));
        }
        Ok(unsigned_res)
    }
}

fn write_bytes(
    store: &mut Store<()>,
    _instance: &Instance,
    alloc: &wasmtime::TypedFunc<i32, i32>,
    memory: wasmtime::Memory,
    text: &str,
) -> Result<(i32, i32)> {
    let bytes = text.as_bytes();
    let len = bytes.len() as i32;
    let ptr = alloc.call(&mut *store, len)?;
    memory.write(&mut *store, ptr as usize, bytes)?;
    Ok((ptr, len))
}
