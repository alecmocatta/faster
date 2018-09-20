// This file is part of faster, the SIMD library for humans.
// Copyright 2017 Adam Niederer <adam.niederer@gmail.com>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::vecs::{Packable, Packed};
use crate::core::slice::from_raw_parts;

pub trait SIMDObject : Sized {
    type Scalar : Packable;
    type Vector : Packed<Scalar = Self::Scalar>;

    /// Return the vector length of this object.
    #[inline(always)]
    fn width(&self) -> usize {
        Self::Vector::WIDTH
    }

    /// Return the scalar size of this object.
    #[inline(always)]
    fn size(&self) -> usize {
        Self::Scalar::SIZE
    }
}

/// An iterator which automatically packs the values it iterates over into SIMD
/// vectors.
pub trait SIMDIterable : SIMDObject + SIMDSized + ExactSizeIterator<Item = <Self as SIMDObject>::Vector> {
    /// Return the current position of this iterator, measured in scalars
    fn scalar_pos(&self) -> usize;

    /// Return the current position of this iterator, measured in vectors.
    #[inline(always)]
    fn vector_pos(&self) -> usize {
        self.scalar_pos() / self.width()
    }

    /// Advance the iterable by `amount` scalars.
    fn advance(&mut self, amount: usize);

    /// Advance the iterable such that it procudes no more items.
    #[inline(always)]
    fn finalize(&mut self) {
        let end = self.scalar_len() - self.scalar_pos();
        self.advance(end);
    }

    /// Return the default vector for this iterable.
    fn default(&self) -> Self::Vector;

    #[inline(always)]
    /// Create a an iterator over the remaining scalar elements in this iterator
    fn unpack(self) -> Unpacked<Self> {
        Unpacked {
            iter: self,
        }
    }

    #[inline(always)]
    /// Create an iterator which returns `amt` vectors at a time.
    fn unroll<'a>(&'a mut self, amt: usize) -> Unrolled<'a, Self> {
        assert!(amt <= 8);
        Unrolled {
            iter: self,
            amt: amt,
            scratch: [<Self as SIMDObject>::Vector::default(); 8]
        }
    }
}

/// An iterator which automatically packs the values it iterates over into SIMD
/// vectors, and can handle collections which do not fit into the system's
/// vectors natively.
pub trait SIMDIterator : SIMDIterable {
    /// Pack and return a partially full vector containing up to the next
    /// `self.width()` of the iterator, or None if no elements are left,
    /// and the number of elements which were not filled. Elements which are
    /// not filled are instead initialized to default.
    fn end(&mut self) -> Option<(Self::Vector, usize)>;

    #[inline(always)]
    /// Return an iterator which calls `func` on vectors of elements.
    fn simd_map<A, B, F>(self, func: F) -> SIMDMap<Self, F>
        where F : FnMut(Self::Vector) -> A, A : Packed<Scalar = B>, B : Packable {
        SIMDMap {
            iter: self,
            func: func,
        }
    }

    #[inline(always)]
    /// Pack and run `func` over the iterator, returning no value and not
    /// modifying the iterator.
    fn simd_do_each<F>(&mut self, mut func: F)
        where F : FnMut(Self::Vector) -> () {
        while let Some(v) = self.next() {
            func(v);
        }
        if let Some((v, _)) = self.end() {
            func(v);
        }
    }

    #[inline(always)]
    /// Return a vector generated by reducing `func` over accumulator `start`
    /// and the values of this iterator, initializing all vectors to `default`
    /// before populating them with elements of the iterator.
    ///
    /// # Examples
    ///
    /// ```
    /// extern crate faster;
    /// use faster::*;
    ///
    /// # fn main() {
    /// let reduced = (&[2.0f32; 100][..]).simd_iter(f32s(0.0))
    ///     .simd_reduce(f32s(0.0), |acc, v| acc + v);
    /// # }
    /// ```
    ///
    /// In this example, on a machine with 4-element vectors, the argument to
    /// the last call of the closure is
    ///
    /// ```rust,ignore
    /// [ 2.0 | 2.0 | 2.0 | 2.0 ]
    /// ```
    ///
    /// and the result of the reduction is
    ///
    /// ```rust,ignore
    /// [ 50.0 | 50.0 | 50.0 | 50.0 ]
    /// ```
    ///
    /// whereas on a machine with 8-element vectors, the last call is passed
    ///
    /// ```rust,ignore
    /// [ 2.0 | 2.0 | 2.0 | 2.0 | 0.0 | 0.0 | 0.0 | 0.0 ]
    /// ```
    ///
    /// and the result of the reduction is
    ///
    /// ```rust,ignore
    /// [ 26.0 | 26.0 | 26.0 | 26.0 | 24.0 | 24.0 | 24.0 | 24.0 ]
    /// ```
    ///
    /// # Footgun Warning
    ///
    /// The results of `simd_reduce` are not portable, and it is your
    /// responsibility to interpret the result in such a way that the it is
    /// consistent across different architectures. See [`Packed::sum`] and
    /// [`Packed::product`] for built-in functions which may be helpful.
    ///
    /// [`Packed::sum`]: vecs/trait.Packed.html#tymethod.sum
    /// [`Packed::product`]: vecs/trait.Packed.html#tymethod.product
    fn simd_reduce<A, F>(&mut self, mut start: A, mut func: F) -> A
        where F : FnMut(A, Self::Vector) -> A {

        while let Some(v) = self.next() {
            start = func(start, v);
        }
        if let Some((v, _)) = self.end() {
            start = func(start, v);
        }
        start
    }
}

/// A trait defining a SIMD iterator over a mutable blob of primitive data
pub trait SIMDIteratorMut : SIMDIterator {
    /// Pack and run `func` over the iterator, modifying each element in-place.
    fn simd_for_each<F>(&mut self, func: F)
        where F : FnMut(&mut Self::Vector) -> ();
}

/// A trait defining a sized blob of primitive data
pub trait SIMDSized : SIMDObject {
    /// Return the length of this iterator, measured in scalars.
    fn scalar_len(&self) -> usize;

    /// Return the length of this iterator, measured in vectors.
    #[inline(always)]
    fn vector_len(&self) -> usize {
        self.scalar_len() / self.width()
    }
}

/// A trait defining a random-access blob of data which can be loaded via SIMD
pub trait SIMDArray : SIMDObject + SIMDSized {
    fn load(&self, offset: usize) -> Self::Vector;
    unsafe fn load_unchecked(&self, offset: usize) -> Self::Vector;
    fn load_scalar(&self, offset: usize) -> Self::Scalar;
    unsafe fn load_scalar_unchecked(&self, offset: usize) -> Self::Scalar;
}

/// A trait defining a random-access mutable blob of data which can be loaded
/// and stored to via SIMD.
pub trait SIMDArrayMut : SIMDArray {
    fn store(&mut self, value: Self::Vector, offset: usize);
    unsafe fn store_unchecked(&mut self, value: Self::Vector, offset: usize);
    fn store_scalar(&mut self, value: Self::Scalar, offset: usize);
    unsafe fn store_scalar_unchecked(&mut self, value: Self::Scalar, offset: usize);
}

/// A slice-backed iterator which can automatically pack its constituent
/// elements into vectors.
#[derive(Clone, Debug)]
pub struct SIMDIter<A : SIMDArray> {
    pub position: usize,
    pub data: A,
    pub default: A::Vector,
}

/// A lazy mapping iterator which applies its function to a stream of vectors.
#[derive(Debug)]
pub struct SIMDMap<I, F> where I : SIMDIterable {
    pub iter: I,
    pub func: F,
}

/// An iterator which packs an iterator of scalars into an iterator of vectors.
/// Cannot take advantage of vectorized loads, so it's very slow to gather data!
#[derive(Clone)]
pub struct SIMDAdapter<I, V> where I : ExactSizeIterator<Item = V::Scalar>, V : Packed {
    pub iter: I,
    pub scratch: V,
    pub default: V,
    pub position: usize,
}

impl<I, V> SIMDObject for SIMDAdapter<I, V> where I : ExactSizeIterator<Item = V::Scalar>, V : Packed {
    type Scalar = V::Scalar;
    type Vector = V;
}

impl<I, V> Iterator for SIMDAdapter<I, V> where I : ExactSizeIterator<Item = V::Scalar>, V : Packed {
    type Item = V;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.position + self.width() <= self.scalar_len() {
            // Our iterator has at least one vector's worth of elements, so load
            // them all into our vector
            for offset in 0..self.width() {
                unsafe {
                    self.scratch = self.scratch.replace_unchecked(offset, self.iter.next().unwrap());
                }
            }
            let width = self.width(); // Appease borrow checker
            self.advance(width);
            Some(self.scratch)
        } else {
            None
        }
    }
}

impl<I, V> SIMDIterator for SIMDAdapter<I, V> where I : ExactSizeIterator<Item = V::Scalar>, V : Packed {
    fn end(&mut self) -> Option<(Self::Vector, usize)> {
        if self.position < self.scalar_len() {
            // This is the last vector we can load, so we should load it
            // backwards and use the default vector to overwrite blank spots
            self.scratch = self.default;
            let mut offset = self.width() - 1;
            while let Some(item) = self.iter.next() {
                unsafe {
                    self.scratch = self.scratch.replace_unchecked(offset, item);
                }
                offset -= 1;
            }
            self.finalize();
            Some((self.scratch, offset))
        } else {
            None
        }
    }
}

impl<I, V> ExactSizeIterator for SIMDAdapter<I, V> where I : ExactSizeIterator<Item = V::Scalar>, V : Packed {
    #[inline(always)]
    fn len(&self) -> usize {
        self.iter.len() / self.width()
    }
}

impl<I, V> SIMDSized for SIMDAdapter<I, V> where I : ExactSizeIterator<Item = V::Scalar>, V : Packed {
    #[inline(always)]
    fn scalar_len(&self) -> usize {
        self.iter.len()
    }
}

impl<I, V> SIMDIterable for SIMDAdapter<I, V> where I : ExactSizeIterator<Item = V::Scalar>, V : Packed {
    #[inline(always)]
    fn scalar_pos(&self) -> usize {
        self.position
    }

    #[inline(always)]
    fn advance(&mut self, amount: usize) {
        self.position += amount
    }

    #[inline(always)]
    fn default(&self) -> Self::Vector {
        self.default
    }
}

impl<'a, S, V> SIMDArrayMut for &'a mut [S] where S : 'a + Packable<Vector = V>, V : Packed<Scalar = S> {
    #[inline(always)]
    fn store(&mut self, value: Self::Vector, offset: usize) {
        value.store(self, offset)
    }

    #[inline(always)]
    unsafe fn store_unchecked(&mut self, value: Self::Vector, offset: usize) {
        value.store_unchecked(self, offset)
    }

    #[inline(always)]
    fn store_scalar(&mut self, value: Self::Scalar, offset: usize) {
        self[offset] = value;
    }

    #[inline(always)]
    unsafe fn store_scalar_unchecked(&mut self, value: Self::Scalar, offset: usize) {
        debug_assert!(offset < self.len());
        *self.get_unchecked_mut(offset) = value;
    }
}

/// A slice-backed iterator which yields scalar elements using the Iterator API.
#[derive(Debug)]
pub struct Unpacked<T> where T : SIMDIterable {
    pub iter: T
}

impl<T> Iterator for Unpacked<T> where T : SIMDIterable + SIMDArray {
    type Item = <T as SIMDObject>::Scalar;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.iter.scalar_pos() < self.iter.scalar_len() {
            let ret = unsafe { self.iter.load_scalar_unchecked(self.iter.scalar_pos()) };
            self.iter.advance(1);
            Some(ret)
        } else {
            None
        }
    }
}

impl<T> Unpacked<T> where T : SIMDIterable {
    #[inline(always)]
    pub fn pack(self) -> T {
        self.iter
    }
}

/// An iterator which yields multiple elements of a PackedIter
#[derive(Debug)]
pub struct Unrolled<'a, T : 'a + SIMDIterable> {
    iter: &'a mut T,
    amt: usize,
    scratch: [T::Vector; 8],
}

impl<'a, T> Unrolled<'a, T> where T : 'a + SIMDIterable {
    #[inline(always)]
    pub fn chunk_len(&self) -> usize {
        self.amt
    }

    #[inline(always)]
    pub fn chunk_pos(&self) -> usize {
        self.iter.vector_pos() / self.chunk_len()
    }
}

impl<'a, T> Iterator for Unrolled<'a, T> where T : 'a + SIMDIterator {
    type Item = &'a [T::Vector];

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let mut i = 0;
        while i < self.amt {
            if let Some(vec) = self.iter.next() {
                self.scratch[i] = vec;
                i += 1;
            } else {
                break;
            }
        }
        if i > 0 {
            unsafe { // TODO: Is this unsafe? Contravariant lifetimes?
                Some(from_raw_parts((&mut self.scratch).as_mut_ptr(), i))
            }
        } else {
            None
        }
    }
}

macro_rules! impl_iter {
    ($name:ty, ($($genera:tt),*) $($pred:tt)*) => {
        impl< $($genera),* > SIMDObject for $name $($pred )* {
            type Vector = V;
            type Scalar = S;
        }

        impl< $($genera),* > SIMDArray for $name $($pred )* {
            #[inline(always)]
            fn load(&self, offset: usize) -> Self::Vector {
                <Self::Vector as Packed>::load(&self, offset)
            }

            #[inline(always)]
            unsafe fn load_unchecked(&self, offset: usize) -> Self::Vector {
                debug_assert!(self[offset..].len() >= Self::Vector::WIDTH);
                <Self::Vector as Packed>::load_unchecked(&self, offset)
            }

            #[inline(always)]
            fn load_scalar(&self, offset: usize) -> Self::Scalar {
                self[offset]
            }

            #[inline(always)]
            unsafe fn load_scalar_unchecked(&self, offset: usize) -> Self::Scalar {
                debug_assert!(offset < self.len());
                *self.get_unchecked(offset)
            }
        }

        impl< $($genera),* > SIMDSized for $name $($pred )* {
            #[inline(always)]
            fn scalar_len(&self) -> usize {
                self.len()
            }
        }
    }
}

#[cfg(feature = "std")]
impl_iter!(Vec<S>, ('a, S, V) where S : Packable<Vector = V>, V : Packed<Scalar = S>);
impl_iter!(&'a [S], ('a, S, V) where S : Packable<Vector = V>, V : Packed<Scalar = S>);
impl_iter!(&'a mut [S], ('a, S, V) where S : Packable<Vector = V>, V : Packed<Scalar = S>);

impl<A> SIMDObject for SIMDIter<A> where A : SIMDArray, A::Vector : Packed, A::Scalar : Packable {
    type Vector = A::Vector;
    type Scalar = A::Scalar;
}

impl<A> ExactSizeIterator for SIMDIter<A> where A : SIMDArray, A::Vector : Packed, A::Scalar : Packable {
    #[inline(always)]
    fn len(&self) -> usize {
        self.data.scalar_len() / self.width()
    }
}

impl<A> Iterator for SIMDIter<A> where A : SIMDArray, A::Vector : Packed, A::Scalar : Packable {
    type Item = <Self as SIMDObject>::Vector;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.position + self.width() <= self.scalar_len() {
            let ret = unsafe { self.load_unchecked(self.position) };
            let width = self.width(); // Appease borrow checker
            self.advance(width);
            Some(ret)
        } else {
            None
        }
    }
}

impl<A> SIMDArray for SIMDIter<A> where A : SIMDArray, A::Vector : Packed, A::Scalar : Packable {
    #[inline(always)]
    fn load(&self, offset: usize) -> Self::Vector {
        self.data.load(offset)
    }

    #[inline(always)]
    unsafe fn load_unchecked(&self, offset: usize) -> Self::Vector {
        self.data.load_unchecked(offset)
    }

    #[inline(always)]
    fn load_scalar(&self, offset: usize) -> Self::Scalar {
        self.data.load_scalar(offset)
    }

    #[inline(always)]
    unsafe fn load_scalar_unchecked(&self, offset: usize) -> Self::Scalar {
        self.data.load_scalar_unchecked(offset)
    }
}

impl<A> SIMDSized for SIMDIter<A> where A : SIMDArray, A::Vector : Packed, A::Scalar : Packable {
    #[inline(always)]
    fn scalar_len(&self) -> usize {
        self.data.scalar_len()
    }
}

impl<A> SIMDIterable for SIMDIter<A> where A : SIMDArray, A::Vector : Packed, A::Scalar : Packable {
    #[inline(always)]
    fn scalar_pos(&self) -> usize {
        self.position
    }

    #[inline(always)]
    fn advance(&mut self, amount: usize) {
        self.position += amount
    }

    #[inline(always)]
    fn default(&self) -> Self::Vector {
        self.default
    }
}

impl<T, S, V> SIMDIterator for T where T : SIMDIterable + SIMDArray<Scalar = S, Vector = V>, S : Packable, V : Packed<Scalar = S> {
    #[inline(always)]
    fn end(&mut self) -> Option<(Self::Vector, usize)> {
        if self.scalar_pos() < self.scalar_len() {
            let mut ret = self.default();
            let empty_amt = self.width() - (self.scalar_len() - self.scalar_pos());
            // Right-align the partial vector to ensure the load is vectorized
            if self.width() < self.scalar_len() {
                ret = unsafe { self.load_unchecked(self.scalar_len() - self.width()) };
                ret = self.default().merge_partitioned(ret, empty_amt);
            } else {
                for i in self.scalar_pos()..self.scalar_len() {
                    unsafe {
                        ret = ret.replace_unchecked(i + empty_amt, self.load_scalar_unchecked(i));
                    }
                }
            }
            self.finalize();
            Some((ret, empty_amt))
        } else {
            None
        }
    }

}

impl<T> SIMDIteratorMut for SIMDIter<T> where T : SIMDArrayMut {
    fn simd_for_each<F>(&mut self, mut func: F)
        where F : FnMut(&mut Self::Vector) -> () {
        let mut lastvec = Self::Vector::default();

        while let Some(mut v) = self.next() {
            func(&mut v);
            lastvec = v;
            let offset = self.scalar_pos() - self.width();
            unsafe { self.data.store_unchecked(v, offset); }
        }
        let offset = self.scalar_pos();
        if let Some((mut p, n)) = self.end() {
            func(&mut p);
            let width = self.width();
            if self.width() < self.scalar_len() {
                // We stored a vector in this buffer; overwrite the unused elements
                unsafe {
                    self.data.store_unchecked(p, offset - n);
                    self.data.store_unchecked(lastvec, offset - width);
                }
            } else {
                // The buffer won't fit one vector; store elementwise
                for i in 0..(width - n) {
                    unsafe { self.data.store_scalar_unchecked(p.extract_unchecked(i + n), offset + i); }
                }
            }
        }
    }
}

#[doc(hidden)]
pub trait UnsafeIterator : Iterator + SIMDIterable {
    unsafe fn next_unchecked(&mut self, offset: usize) -> Self::Item;
    unsafe fn end_unchecked(&mut self, offset: usize, empty_amt: usize) -> Self::Vector;
}

impl<T, S, V> UnsafeIterator for T where T : SIMDIterable + SIMDArray<Scalar = S, Vector = V>, S : Packable, V : Packed<Scalar = S> {
    #[inline(always)]
    unsafe fn next_unchecked(&mut self, offset: usize) -> Self::Item {
        debug_assert!(offset + self.width() <= self.scalar_len());
        self.load_unchecked(offset)
    }

    #[inline(always)]
    unsafe fn end_unchecked(&mut self, offset: usize, empty_amt: usize) -> Self::Vector {
        debug_assert!(offset < self.scalar_len());
        let mut ret = self.default();
        debug_assert_eq!(empty_amt, self.width() - (self.scalar_len() - offset));
        // Right-align the partial vector to ensure the load is vectorized
        if self.width() < self.scalar_len() {
            ret = self.load_unchecked(self.scalar_len() - self.width());
            ret = self.default().merge_partitioned(ret, empty_amt);
        } else {
            for i in offset..self.scalar_len() {
                ret = ret.replace_unchecked(i + empty_amt, self.load_scalar_unchecked(i));
            }
        }
        ret
    }

}

impl<A, B, I, F> Iterator for SIMDMap<I, F>
    where I : SIMDIterable, F : FnMut(I::Vector) -> A, A : Packed<Scalar = B>, B : Packable {
    type Item = A;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(&mut self.func)
    }
}

impl<I, F> ExactSizeIterator for SIMDMap<I, F> where Self : Iterator, I : SIMDIterable {
    #[inline(always)]
    fn len(&self) -> usize {
        self.iter.len()
    }
}

impl<A, B, I, F> SIMDObject for SIMDMap<I, F>
    where I : SIMDIterable, F : FnMut(I::Vector) -> A, A : Packed<Scalar = B>, B : Packable {
    type Vector = A;
    type Scalar = B;

    #[inline(always)]
    fn width(&self) -> usize {
        Self::Vector::WIDTH
    }
}

impl<A, B, I, F> SIMDSized for SIMDMap<I, F>
    where I : SIMDIterable, F : FnMut(I::Vector) -> A, A : Packed<Scalar = B>, B : Packable {
    #[inline(always)]
    fn scalar_len(&self) -> usize {
        self.iter.len()
    }
}

impl<A, B, I, F> SIMDIterable for SIMDMap<I, F>
    where I : SIMDIterable, F : FnMut(I::Vector) -> A, A : Packed<Scalar = B>, B : Packable {
    #[inline(always)]
    fn scalar_pos(&self) -> usize {
        self.iter.scalar_pos()
    }

    #[inline(always)]
    fn advance(&mut self, amount: usize) {
        self.iter.advance(amount);
    }

    #[inline(always)]
    fn default(&self) -> Self::Vector {
        // TODO: Is there a more sane return value (without invoking the closure)?
        <Self::Vector as Packed>::default()
    }
}

impl<'a, A, B, I, F> SIMDIterator for SIMDMap<I, F>
    where I : SIMDIterator, F : FnMut(I::Vector) -> A, A : Packed<Scalar = B>, B : Packable {
    #[inline(always)]
    fn end(&mut self) -> Option<(Self::Vector, usize)> {
        let (v, n) = self.iter.end()?;
        let nr = n * I::Scalar::SIZE / Self::Scalar::SIZE;
        Some(((self.func)(v), nr))
    }
}

/// A trait which can transform a stream of vectors into a contiguous
/// collection of scalars.
pub trait IntoScalar<T> : SIMDObject where T : Packable {
    /// Take an iterator of SIMD vectors, and store them in-order in a Vec.
    #[cfg(feature = "std")]
    fn scalar_collect(&mut self) -> Vec<T>;

    /// Take an iterator of SIMD vectors and store them in-order in `fill`.
    fn scalar_fill<'a>(&mut self, fill: &'a mut [T]) -> &'a mut [T];

    /// Take an iterator of SIMD vectors, and store them in-order in a Vec,
    /// including possibly redundant elements at the end of the iterator.
    #[cfg(feature = "std")]
    fn scalar_collect_all(&mut self) -> Vec<T>;

    /// Take an iterator of SIMD vectors and store them in-order in `fill`,
    /// including possibly redundant elements at the end of the iterator.
    fn scalar_fill_all<'a>(&mut self, fill: &'a mut [T]) -> &'a mut [T];
}

impl<'a, T, I> IntoScalar<T> for I
    where I : SIMDIterator<Scalar = T>, I::Vector : Packed<Scalar = T>, T : Packable {

    #[inline(always)]
    #[cfg(feature = "std")]
    fn scalar_collect(&mut self) -> Vec<Self::Scalar> {
        let mut ret = Vec::with_capacity((self.len() + 1) * self.width());
        let mut offset = 0;
        let mut lastvec = Self::Vector::default();

        unsafe {
            ret.set_len((self.len() + 1) * self.width());
            while let Some(vec) = self.next() {
                vec.store_unchecked(&mut ret, offset);
                offset += self.width();
                lastvec = vec;
            }

            if let Some((p, n)) = self.end() {
                if offset > 0 {
                    // We stored a vector in this buffer; overwrite the unused elements
                    p.store_unchecked(&mut ret, offset - n);
                    lastvec.store_unchecked(&mut ret, offset - self.width());
                } else {
                    // The buffer won't fit one vector; store elementwise
                    for i in 0..(self.width() - n) {
                        ret[offset + i] = p.extract_unchecked(i + n);
                    }
                }
                ret.set_len(self.width() + offset - n);
            } else {
                ret.set_len(self.len() * self.width());
            }
        }
        ret
    }

    #[inline(always)]
    fn scalar_fill<'b>(&mut self, fill: &'b mut [Self::Scalar]) -> &'b mut [Self::Scalar] {
        let mut offset = 0;
        let mut lastvec = Self::Vector::default();

        while let Some(vec) = self.next() {
            unsafe { vec.store_unchecked(fill, offset); }
            offset += self.width();
            lastvec = vec;
        }

        if let Some((p, n)) = self.end() {
            if offset > 0 {
                // We stored a vector in this buffer; overwrite the unused elements
                unsafe {
                    p.store_unchecked(fill, offset - n);
                    lastvec.store_unchecked(fill, offset - self.width());
                }
            } else {
                // The buffer won't fit one vector; store elementwise
                for i in 0..(self.width() - n) {
                    unsafe {
                        fill[offset + i] = p.extract_unchecked(i + n);
                    }
                }
            }
        }

        fill
    }

    #[inline(always)]
    #[cfg(feature = "std")]
    fn scalar_collect_all(&mut self) -> Vec<Self::Scalar> {
        let mut ret = Vec::with_capacity((self.len() + 1) * self.width());

        unsafe {
            ret.set_len(self.len());
            self.scalar_fill_all(ret.as_mut_slice());
        }
        ret
    }

    #[inline(always)]
    fn scalar_fill_all<'b>(&mut self, fill: &'b mut [Self::Scalar]) -> &'b mut [Self::Scalar] {
        let mut offset = 0;

        while let Some(vec) = self.next() {
            unsafe { vec.store_unchecked(fill, offset); }
            offset += self.width();
        }

        if let Some((vec, _)) = self.end() {
            unsafe { vec.store_unchecked(fill, offset); }
        }

        fill
    }
}

mod tests {
    #[allow(unused_imports)] // WTF?
    use crate::prelude::*;

    #[test]
    #[cfg(feature = "std")]
    fn bitcast_map_width_doubles() {
        let y = [1, 2, 3, 4, 5i64].simd_iter(i64s(0))
            .simd_map(|v| v.to_le().be_u32s())
            .scalar_collect();

        assert_eq!(y, [1, 0, 2, 0, 3, 0, 4, 0, 5, 0]);
    }

    #[test]
    #[cfg(feature = "std")]
    fn bitcast_map_width_quadruples() {
        let y = [1, 2, 3, 4, 5i64].simd_iter(i64s(0))
            .simd_map(|v| v.to_le().be_u16s())
            .scalar_collect();

        assert_eq!(y, [1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0,
                       4, 0, 0, 0, 5, 0, 0, 0]);
    }

    #[test]
    #[cfg(feature = "std")]
    fn bitcast_map_width_octuples() {
        let y = [1, 2, 3, 4, 5i64].simd_iter(i64s(0))
            .simd_map(|v| v.to_le().be_u8s())
            .scalar_collect();

        assert_eq!(y.as_slice(),
                   &[1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0,
                     3, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0,
                     5, 0, 0, 0, 0, 0, 0, 0u8][..]);
    }
}
