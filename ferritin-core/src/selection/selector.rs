//! Provides the `AtomSelector` type for making atom selections from a protein structure.
//!
//! This module implements a builder pattern for filtering and selecting atoms based on
//! various criteria like chain ID, residue name, element type, spatial location, etc.
//!
//! # Example
//!
//! ```no_run
//! use ferritin_core::AtomCollection;
//! # fn example(atoms: &AtomCollection) {
//! let selection = atoms.select()
//!     .chain("A")                // Select chain A
//!     .residue("ALA")           // Filter to alanine residues
//!     .sphere([0.0, 0.0, 0.0], 10.0)  // Within 10Å of origin
//!     .collect();               // Get the selected atoms
//! # }
//! ```
use super::selection::Selection;
use super::view::AtomView;
use crate::AtomCollection;
use pdbtbx::Element;

/// A structure for selecting atoms from an `AtomCollection` using various filtering criteria.
///
/// The `AtomSelector` provides a builder-style interface for creating atom selections through
/// methods like `chain()`, `element()`, `residue()`, etc. Each method returns `Self` for
/// method chaining.
pub struct AtomSelector<'a> {
    /// Reference to the underlying atom collection being selected from
    collection: &'a AtomCollection,
    /// The current selection state tracking which atoms are selected
    current_selection: Selection,
}

impl<'a> AtomSelector<'a> {
    pub(crate) fn new(collection: &AtomCollection) -> AtomSelector<'_> {
        let size = collection.get_size();
        AtomSelector {
            collection,
            current_selection: Selection::new((0..size).collect()),
        }
    }
    pub fn chain(mut self, chain_id: &str) -> Self {
        let chain_selection = self.collection.select_by_chain(chain_id);
        self.current_selection = &self.current_selection & &chain_selection;
        self
    }
    pub fn collect(self) -> AtomView<'a> {
        AtomView::new(self.collection, self.current_selection)
    }

    pub fn element(mut self, element: Element) -> Self {
        let element_selection = self
            .collection
            .get_elements()
            .iter()
            .enumerate()
            .filter(|(_, &e)| e == element)
            .map(|(i, _)| i)
            .collect();
        self.current_selection = &self.current_selection & &Selection::new(element_selection);
        self
    }
    pub fn filter<F>(mut self, predicate: F) -> Self
    where
        F: Fn(usize) -> bool,
    {
        let filtered = self
            .current_selection
            .indices
            .iter()
            .filter(|&&idx| predicate(idx))
            .copied()
            .collect();
        self.current_selection = Selection::new(filtered);
        self
    }
    pub fn filter_backbone(self) -> Self {
        unimplemented!()
    }
    pub fn filter_protein(self) -> Self {
        unimplemented!()
    }
    pub fn filter_polymer(self) -> Self {
        unimplemented!()
    }
    pub fn filter_solvent(self) -> Self {
        unimplemented!()
    }
    pub fn residue(mut self, res_name: &str) -> Self {
        let res_selection = self.collection.select_by_residue(res_name);
        self.current_selection = &self.current_selection & &res_selection;
        self
    }

    pub fn sphere(mut self, center: [f32; 3], radius: f32) -> Self {
        let sphere_selection = self
            .collection
            .get_coords()
            .iter()
            .enumerate()
            .filter(|(_, &pos)| {
                let dx = pos[0] - center[0];
                let dy = pos[1] - center[1];
                let dz = pos[2] - center[2];
                (dx * dx + dy * dy + dz * dz).sqrt() <= radius
            })
            .map(|(i, _)| i)
            .collect();
        self.current_selection = &self.current_selection & &Selection::new(sphere_selection);
        self
    }
}
