//! ESM2 Tokenizer. Models converted to ONNX format from [ESM2](https://github.com/facebookresearch/esm)
//! and uploaded to HuggingFace hub. The tokenizer is included in this crate and loaded from
//! memory using `tokenizer.json`. This is fairly minimal - for the full set of ESM2 models
//! please see the ESM2 repository and the HuggingFace hub.
//!
//! # Models:
//! * ESM2_T6_8M - small 6-layer protein language model
//! * ESM2_T12_35M - medium 12-layer protein language model
//! * ESM2_T30_150M - large 30-layer protein language model
//!
use super::super::utilities::ndarray_to_tensor_f32;
use anyhow::{anyhow, Result};
use candle_core::{Tensor, D};
use candle_nn::ops;
use ferritin_plms::types::PseudoProbability;
use hf_hub::api::sync::Api;
use ndarray::Array2;
use ort::{
    execution_providers::CUDAExecutionProvider,
    session::{
        builder::{GraphOptimizationLevel, SessionBuilder},
        Session,
    },
};
use std::path::PathBuf;
use tokenizers::Tokenizer;

pub enum ESM2Models {
    ESM2_T6_8M,
    ESM2_T12_35M,
    ESM2_T30_150M,
    // ESM2_T33_650M,
}

pub struct ESM2 {
    session: SessionBuilder,
    model_path: PathBuf,
    tokenizer: Tokenizer,
}

impl ESM2 {
    pub fn new(model: ESM2Models) -> Result<Self> {
        let session = Self::create_session()?;
        let model_path = Self::load_model_path(model)?;
        let tokenizer = Self::load_tokenizer()?;
        Ok(Self {
            session,
            model_path,
            tokenizer,
        })
    }
    pub fn load_model_path(model: ESM2Models) -> Result<PathBuf> {
        let api = Api::new()?;
        let repo_id = match model {
            ESM2Models::ESM2_T6_8M => "zcpbx/esm2-t6-8m-UR50D-onnx",
            ESM2Models::ESM2_T12_35M => "zcpbx/esm2-t12-35M-UR50D-onnx",
            ESM2Models::ESM2_T30_150M => "zcpbx/esm2-t30-150M-UR50D-onnx",
            // ESM2Models::ESM2_T33_650M => "zcpbx/esm2-t33-650M-UR50D-onnx",
        }
        .to_string();
        let model_path = api.model(repo_id).get("model.onnx")?;
        Ok(model_path)
    }
    pub fn load_tokenizer() -> Result<Tokenizer> {
        let tokenizer_bytes = include_bytes!("tokenizer.json");
        Tokenizer::from_bytes(tokenizer_bytes)
            .map_err(|e| anyhow!("Failed to load tokenizer: {}", e))
    }
    fn create_session() -> Result<SessionBuilder> {
        ort::init()
            .with_name("ESM2")
            .with_execution_providers([CUDAExecutionProvider::default().build()])
            .commit()?;
        Ok(Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level1)?
            .with_intra_threads(1)?)
    }
    pub fn run_model(&self, sequence: &str) -> Result<Tensor> {
        let model = self.session.clone().commit_from_file(&self.model_path)?;
        let tokens = self
            .tokenizer
            .encode(sequence, false)
            .map_err(|e| anyhow!("Tokenization failed: {}", e))?;
        let token_ids = tokens.get_ids();
        let shape = (1, tokens.len());
        // Todo: Are we masking this correctly?
        let mask_array: Array2<i64> = Array2::from_shape_vec(shape, vec![1; tokens.len()])?;
        let tokens_array: Array2<i64> = Array2::from_shape_vec(
            shape,
            token_ids.iter().map(|&x| x as i64).collect::<Vec<_>>(),
        )?;
        let outputs =
            model.run(ort::inputs!["input_ids" => tokens_array,"attention_mask" => mask_array]?)?;
        let logits = outputs["logits"].try_extract_tensor::<f32>()?.to_owned();
        Ok(ndarray_to_tensor_f32(logits)?)
    }
    // Softmax and simplify
    pub fn extract_logits(&self, tensor: &Tensor) -> Result<Vec<PseudoProbability>> {
        let tensor = ops::softmax(tensor, D::Minus1)?;
        let data = tensor.to_vec3::<f32>()?;
        println!("Data: {:?}", data);
        let shape = tensor.dims();
        let mut logit_positions = Vec::new();
        for seq_pos in 0..shape[1] {
            for vocab_idx in 0..shape[2] {
                let score = data[0][seq_pos][vocab_idx];
                let amino_acid_char = self
                    .tokenizer
                    .decode(&[vocab_idx as u32], false)
                    .map_err(|e| anyhow!("Failed to decode: {}", e))?
                    .chars()
                    .next()
                    .ok_or_else(|| anyhow!("Empty decoded string"))?;
                logit_positions.push(PseudoProbability {
                    position: seq_pos,
                    amino_acid: amino_acid_char,
                    pseudo_prob: score,
                });
            }
        }
        Ok(logit_positions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tokenizer_load() -> Result<()> {
        let tokenizer = ESM2::load_tokenizer()?;
        let text = "MLKLRV";
        let encoding = tokenizer
            .encode(text, false)
            .map_err(|e| anyhow!("Failed to encode: {}", e))?;
        let tokens = encoding.get_tokens();
        assert_eq!(tokens.len(), 6);
        assert_eq!(tokens, &["M", "L", "K", "L", "R", "V"]);
        Ok(())
    }
}
