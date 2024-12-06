// use std::ops::Bound;

// use unicode_segmentation::UnicodeSegmentation;
// use unicode_width::UnicodeWidthStr;

// use super::VariableSizeBuffer;

// pub struct Editable {
//     contents: String,
//     grapheme_boundaries: Vec<usize>,
// }

// impl VariableSizeBuffer for Editable {
//     type Item<'a>
//         = &'a str
//     where
//         Self: 'a;

//     fn count(&self) -> u32 {
//         self.grapheme_boundaries.len() as _
//     }

//     fn items(
//         &self,
//         range: impl std::ops::RangeBounds<u32>,
//     ) -> impl ExactSizeIterator<Item = Self::Item<'_>> + DoubleEndedIterator + '_ {
//         let start = match range.start_bound() {
//             Bound::Included(&start) => start as usize,
//             Bound::Excluded(&start) => start as usize + 1,
//             Bound::Unbounded => 0,
//         };
//         let end = match range.end_bound() {
//             Bound::Included(&end) => end as usize + 1,
//             Bound::Excluded(&end) => end as usize,
//             Bound::Unbounded => self.grapheme_boundaries.len(),
//         };

//         self.grapheme_boundaries[start..=end]
//             .windows(2)
//             .map(|pair| &self.contents[pair[0]..pair[1]])
//     }

//     fn size(item: &Self::Item<'_>) -> usize {
//         item.width()
//     }
// }

// impl From<String> for Editable {
//     fn from(contents: String) -> Self {
//         let mut grapheme_boundaries = contents
//             .grapheme_indices(true)
//             .map(|(idx, _)| idx)
//             .collect::<Vec<usize>>();
//         grapheme_boundaries.push(contents.len());
//         Self {
//             contents,
//             grapheme_boundaries,
//         }
//     }
// }

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_new_editable() {
//         let editable = Editable::from("देवनागरी".to_owned());
//         println!("{:?}", editable.count());
//         println!("{:?}", editable.contents);
//         println!("{:?}", editable.grapheme_boundaries);
//         assert!(false)
//     }
// }
