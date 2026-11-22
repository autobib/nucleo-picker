use super::{Components, ItemRef, Matches};
use crate::Generation;

/// Read-only access to match engine and runner context.
///
/// This struct is passed as an argument to the [`Interface`](crate::interface::Interface) and provides access
/// to:
///
/// - Match list information ([`matches`](Self::matches) [`hovered`](Self::hovered), [`hovered_rank`](Self::hovered_rank).
/// - Information about component visibility ([`active_components`](Self::active_components))
pub struct Context<'items, T> {
    matches: Matches<'items, T>,
    rank: Option<u32>,
    active_components: Components,
}

impl<'items, T: Send + Sync + 'static> Context<'items, T> {
    /// Returns the generation of the underlying item pool.
    ///
    /// See the [`Generation`] docs for more detail.
    #[must_use]
    #[inline]
    pub const fn generation(&self) -> Generation {
        self.matches.generation()
    }

    /// Initialize from a snapshot of [`Matches`] from a picker.
    ///
    /// The rank is initially set to `None` and all components are recorded as hidden. Set these
    /// values using [`with_rank`](Self::with_rank) and
    /// [`with_active_components`](Self::with_active_components).
    #[must_use]
    #[inline]
    pub(crate) fn new(matches: Matches<'items, T>) -> Self {
        Self {
            matches,
            rank: None,
            active_components: Components::NONE,
        }
    }

    /// Set the rank to a given index.
    #[must_use]
    #[inline]
    pub(crate) fn with_rank(mut self, rank: Option<u32>) -> Self {
        self.rank = rank.filter(|&rank| (rank as usize) < self.matches.len());
        self
    }

    #[must_use]
    #[inline]
    pub(crate) fn with_active_components(mut self, components: Components) -> Self {
        self.active_components = components;
        self
    }

    /// Return a view of the currently matching items in the picker.
    ///
    /// The returned [`Matches`] struct has lifetime tied to the underlying item storage and can be used to
    /// access picker items and iterate over the matches in this context.
    #[must_use]
    #[inline]
    pub fn matches(&self) -> Matches<'items, T> {
        self.matches
    }

    /// Returns the hovered item, or `None` if the match list is empty.
    pub fn hovered(&self) -> Option<ItemRef<'items, T>> {
        self.matches.get(self.rank? as usize)
    }

    /// Returns the rank of the hovered item, or none if the match list is empty.
    pub fn hovered_rank(&self) -> Option<u32> {
        self.rank
    }

    /// Returns the set of active components.
    ///
    /// The interface may use this to elide background work. If a component is not active, this
    /// only means that it is not drawn to the screen. The internal data may still be used in other
    /// ways.
    pub fn active_components(&self) -> Components {
        self.active_components
    }
}
