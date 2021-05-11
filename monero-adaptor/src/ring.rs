use std::ops::Index;

use curve25519_dalek::edwards::EdwardsPoint;

#[derive(Clone)]
pub struct Ring<'a> {
    elements: &'a [EdwardsPoint; 11],
    bytes: [u8; 32 * 11],
}

impl<'a> Ring<'a> {
    pub fn new(elements: &[EdwardsPoint; 11]) -> Ring<'_> {
        let mut bytes = [0u8; 32 * 11];

        for (i, element) in elements.iter().enumerate() {
            let start = i * 32;
            let end = (i + 1) * 32;

            bytes[start..end].copy_from_slice(element.compress().as_bytes());
        }

        Ring { elements, bytes }
    }
}

impl<'a> AsRef<[u8]> for Ring<'a> {
    fn as_ref(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}

impl<'a> Index<usize> for Ring<'a> {
    type Output = EdwardsPoint;

    fn index(&self, index: usize) -> &Self::Output {
        &self.elements[index]
    }
}
