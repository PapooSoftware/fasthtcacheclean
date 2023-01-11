// Copyright (c) 2022 Christoph Grenz (Papoo Software & Media GmbH) <info@papoo.de>
// SPDX-License-Identifier: LGPL-3.0-or-later

use std::cmp::Ord;
use std::collections::BinaryHeap;

#[derive(Debug, Clone)]
pub struct CachePriorityQueue<T> {
	heap: BinaryHeap<T>,
	limit: usize,
}

#[allow(dead_code)]
impl<T> CachePriorityQueue<T> {
	#[inline]
	pub fn len(&self) -> usize {
		self.heap.len()
	}

	#[inline]
	pub fn clear(&mut self) {
		self.heap.clear()
	}
}

/// A priority queue that keeps a limited amount of items.
///
/// Discards the largest (according to the [`Ord`] trait) items if the limit is reached.
///
/// Implemented with a [`BinaryHeap`].
#[allow(dead_code)]
impl<T: Ord> CachePriorityQueue<T> {
	/// Creates an empty queue that keeps at most `limit` items.
	#[must_use]
	pub fn new(limit: usize) -> Self {
		Self {
			heap: BinaryHeap::new(),
			limit,
		}
	}

	/// Creates an empty queue with a specific capacity.
	///
	/// This preallocates enough memory for `capacity` elements.
	///
	/// # Panics
	/// Panics if `capacity` is larger than `limit`.
	#[must_use]
	pub fn with_capacity(capacity: usize, limit: usize) -> Self {
		assert!(capacity <= limit);
		Self {
			heap: BinaryHeap::with_capacity(capacity),
			limit,
		}
	}

	/// Pushes an item into the queue.
	///
	/// If the limit is reached, the smallest item is removed from the queue
	/// before pushing the new `item`. If the new item is larger than any
	/// item in the queue, it is dropped instead.
	#[inline]
	pub fn push(&mut self, item: T) {
		// If the limit is reached
		if self.heap.len() >= self.limit {
			// If the new element would be the one we would pop(), don't insert
			if let Some(element) = self.heap.peek() {
				if &item > element {
					return;
				}
			}
			// Otherwise pop() one before inserting
			self.heap.pop();
		}
		// Insert new element
		self.heap.push(item);
	}

	/// Consumes the `CachePriorityQueue` and returns a vector in sorted (ascending) order.
	#[inline]
	pub fn into_sorted_vec(self) -> Vec<T> {
		self.heap.into_sorted_vec()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_queue() {
		let mut h = CachePriorityQueue::with_capacity(4, 4);
		h.push(5);
		h.push(7);
		h.push(1);
		assert_eq!(h.len(), 3);
		assert_eq!(&h.into_sorted_vec(), &[1, 5, 7]);

		let mut h = CachePriorityQueue::new(2);
		h.push(5);
		h.push(7);
		h.push(1);
		h.push(9);
		assert_eq!(h.len(), 2);
		assert_eq!(&h.into_sorted_vec(), &[1, 5]);

		let mut h = CachePriorityQueue::new(2);
		h.push(5);
		h.clear();
		assert_eq!(h.len(), 0);
		assert_eq!(&h.into_sorted_vec(), &[]);
	}
}
