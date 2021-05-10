use std::ops::Index;

pub struct Ring<T> {
    elements: [T; 11],
    bytes: [u8; 32 * 11],
}

impl<T> Ring<T> {
    pub fn new(elements: [T; 11], serialize_element: impl Fn(&T) -> &[u8; 32]) -> Ring<T> {
        let mut bytes = [0u8; 32 * 11];

        for (i, element) in elements.iter().enumerate() {
            let start = i * 32;
            let end = (i + 1) * 32;

            bytes[start..end].copy_from_slice(serialize_element(element));
        }

        Ring { elements, bytes }
    }
}

impl<T> AsRef<[u8]> for Ring<T> {
    fn as_ref(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}

impl<T> Index<usize> for Ring<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.elements[index]
    }
}
