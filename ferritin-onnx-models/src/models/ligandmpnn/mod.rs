//! Module for running Ligand- and Protein-MPNN Models
//!
//! This module provides functionality for running LigandMPNN and ProteinMPNN models
//! to predict amino acid sequences given protein structure coordinates and ligand information.
//!
//! The models are loaded from the Hugging Face model hub and executed using ONNX Runtime.
//!
//!
use crate::{ndarray_to_tensor_f32, tensor_to_ndarray_f32, tensor_to_ndarray_i64};
use anyhow::Result;
use candle_core::{Device, Tensor};
use candle_nn::ops;
use ferritin_core::{AtomCollection, StructureFeatures};
use ferritin_plms::ligandmpnn::utilities::int_to_aa1;
use ferritin_plms::types::PseudoProbability;
use hf_hub::api::sync::Api;
use ndarray::ArrayBase;
use ort::{
    execution_providers::CUDAExecutionProvider,
    session::{
        builder::{GraphOptimizationLevel, SessionBuilder},
        Session,
    },
};
use std::path::PathBuf;

type NdArrayF32 = ArrayBase<ndarray::OwnedRepr<f32>, ndarray::Dim<ndarray::IxDynImpl>>;
type NdArrayI64 = ArrayBase<ndarray::OwnedRepr<i64>, ndarray::Dim<ndarray::IxDynImpl>>;

pub enum ModelType {
    Protein,
    Ligand,
}

impl ModelType {
    pub fn get_paths(&self) -> (&'static str, &'static str, &'static str) {
        match self {
            ModelType::Protein => (
                "zcpbx/proteinmpnn-v48-030-onnx",
                "protmpnn_encoder.onnx",
                "protmpnn_decoder_step.onnx",
            ),
            ModelType::Ligand => (
                "zcpbx/ligandmpnn-v32-030-25-onnx",
                "ligand_encoder.onnx",
                "ligand_decoder.onnx",
            ),
        }
    }
}

pub struct LigandMPNN {
    session: SessionBuilder,
    encoder_path: PathBuf,
    decoder_path: PathBuf,
}

impl LigandMPNN {
    pub fn new() -> Result<Self> {
        let session = Self::create_session()?;
        let (encoder_path, decoder_path) = Self::load_model_paths(ModelType::Ligand)?;
        Ok(Self {
            session,
            encoder_path,
            decoder_path,
        })
    }
    fn create_session() -> Result<SessionBuilder> {
        ort::init()
            .with_name("LigandMPNN")
            .with_execution_providers([CUDAExecutionProvider::default().build()])
            .commit()?;
        Ok(Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level1)?
            .with_intra_threads(1)?)
    }
    fn load_model_paths(model_type: ModelType) -> Result<(PathBuf, PathBuf)> {
        let api = Api::new()?;
        let (repo_id, encoder_name, decoder_name) = model_type.get_paths();
        Ok((
            api.model(repo_id.to_string()).get(&encoder_name)?,
            api.model(repo_id.to_string()).get(&decoder_name)?,
        ))
    }
    pub fn run_model(&self, ac: AtomCollection, position: i64, temperature: f32) -> Result<Tensor> {
        let (h_V, h_E, E_idx) = self.run_encoder(&ac)?;
        self.run_decoder(h_V, h_E, E_idx, temperature, position)
    }
    pub fn run_encoder(&self, ac: &AtomCollection) -> Result<(NdArrayF32, NdArrayF32, NdArrayI64)> {
        let device = Device::Cpu;
        let encoder_model = self.session.clone().commit_from_file(&self.encoder_path)?;
        let x_bb = ac.to_numeric_backbone_atoms(&device)?;
        let (lig_coords, lig_elements, lig_mask) = ac.to_numeric_ligand_atoms(&device)?;
        let coords_nd = tensor_to_ndarray_f32(x_bb)?;
        let lig_coords_nd = tensor_to_ndarray_f32(lig_coords)?;
        let lig_types_nd = tensor_to_ndarray_i64(lig_elements)?;
        let lig_mask_nd = tensor_to_ndarray_f32(lig_mask)?;
        let encoder_inputs = ort::inputs![
            "coords" => coords_nd,
            "ligand_coords" => lig_coords_nd,
            "ligand_types" => lig_types_nd,
            "ligand_mask" => lig_mask_nd
        ]?;
        let encoder_outputs = encoder_model.run(encoder_inputs)?;
        Ok((
            encoder_outputs["h_V"]
                .try_extract_tensor::<f32>()?
                .to_owned(),
            encoder_outputs["h_E"]
                .try_extract_tensor::<f32>()?
                .to_owned(),
            encoder_outputs["E_idx"]
                .try_extract_tensor::<i64>()?
                .to_owned(),
        ))
    }
    pub fn run_decoder(
        &self,
        h_V: NdArrayF32,
        h_E: NdArrayF32,
        E_idx: NdArrayI64,
        temperature: f32,
        position: i64,
    ) -> Result<Tensor> {
        let decoder_model = self.session.clone().commit_from_file(&self.decoder_path)?;
        let position_tensor =
            ort::value::Tensor::from_array(ndarray::Array::from_shape_vec([1], vec![position])?)?;
        let temp_tensor = ort::value::Tensor::from_array(ndarray::Array::from_shape_vec(
            [1],
            vec![temperature],
        )?)?;
        let decoder_inputs = ort::inputs![
            "h_V" => h_V,
            "h_E" => h_E,
            "E_idx" => E_idx,
            "position" => position_tensor,
            "temperature" => temp_tensor,
        ]?;

        let decoder_outputs = decoder_model.run(decoder_inputs)?;
        let logits = decoder_outputs["logits"]
            .try_extract_tensor::<f32>()?
            .to_owned();
        ndarray_to_tensor_f32(logits)
    }
    pub fn get_single_location(
        &self,
        ac: AtomCollection,
        temp: f32,
        position: i64,
    ) -> Result<Vec<PseudoProbability>> {
        let logits = self.run_model(ac, position, temp)?;
        let logits = ops::softmax(&logits, 1)?;
        let logits = logits.get(0)?.to_vec1()?;
        let mut amino_acid_probs = Vec::new();
        for i in 0..21 {
            amino_acid_probs.push(PseudoProbability {
                amino_acid: int_to_aa1(i),
                pseudo_prob: logits[i as usize],
                position: position as usize,
            });
        }
        Ok(amino_acid_probs)
    }
    pub fn get_all_locations(&self, temp: f32) -> Result<Vec<PseudoProbability>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritin_test_data::TestFile;
    use pdbtbx;

    fn setup_test_data() -> AtomCollection {
        let (protfile, _handle) = TestFile::protein_01().create_temp().unwrap();
        let (pdb, _) = pdbtbx::open(protfile).expect("PDB/CIF");
        AtomCollection::from(&pdb)
    }

    #[test]
    fn test_model_initialization() {
        let model = LigandMPNN::new().unwrap();
        assert!(model.encoder_path.exists());
        assert!(model.decoder_path.exists());
    }

    #[test]
    fn test_encoder_output_dimensions() {
        let model = LigandMPNN::new().unwrap();
        let ac = setup_test_data();
        let (h_v, h_e, e_idx) = model.run_encoder(&ac).unwrap();
        assert_eq!(h_v.shape(), &[1, 154, 128]);
        assert_eq!(h_e.shape(), &[1, 154, 16, 128]);
        assert_eq!(e_idx.shape(), &[1, 154, 16]);
    }

    #[test]
    fn test_full_pipeline() {
        let model = LigandMPNN::new().unwrap();
        let ac = setup_test_data();
        let logits = model.run_model(ac, 10, 0.1).unwrap();
        assert_eq!(logits.dims2().unwrap(), (1, 21));
    }
}
