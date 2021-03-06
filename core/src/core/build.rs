// Copyright 2016 The Grin Developers
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Utility functions to build Grin transactions. Handles the blinding of
//! inputs and outputs, maintaining the sum of blinding factors, producing
//! the excess signature, etc.
//!
//! Each building function is a combinator that produces a function taking
//! a transaction a sum of blinding factors, to return another transaction
//! and sum. Combinators can then be chained and executed using the
//! _transaction_ function.
//!
//! Example:
//! build::transaction(vec![input_rand(75), output_rand(42), output_rand(32),
//!   with_fee(1)])

use byteorder::{ByteOrder, BigEndian};
use secp;

use core::{Transaction, Input, Output, DEFAULT_OUTPUT};
use keychain;
use keychain::{Keychain, BlindSum, BlindingFactor, Identifier};

/// Context information available to transaction combinators.
pub struct Context<'a> {
	keychain: &'a Keychain,
}

/// Function type returned by the transaction combinators. Transforms a
/// (Transaction, BlindSum) pair into another, provided some context.
type Append = for<'a> Fn(&'a mut Context, (Transaction, BlindSum)) -> (Transaction, BlindSum);

/// Adds an input with the provided value and blinding key to the transaction
/// being built.
pub fn input(value: u64, pubkey: Identifier) -> Box<Append> {
	Box::new(move |build, (tx, sum)| -> (Transaction, BlindSum) {
		let commit = build.keychain.commit(value, &pubkey).unwrap();
		(tx.with_input(Input(commit)), sum.sub_pubkey(pubkey.clone()))
	})
}

/// Adds an output with the provided value and blinding key to the transaction
/// being built.
pub fn output(value: u64, pubkey: Identifier) -> Box<Append> {
	Box::new(move |build, (tx, sum)| -> (Transaction, BlindSum) {
		let commit = build.keychain.commit(value, &pubkey).unwrap();
		let msg = secp::pedersen::ProofMessage::empty();
		let rproof = build.keychain.range_proof(value, &pubkey, commit, msg).unwrap();

		(tx.with_output(Output {
			features: DEFAULT_OUTPUT,
			commit: commit,
			proof: rproof,
		}), sum.add_pubkey(pubkey.clone()))
	})
}

/// Sets the fee on the transaction being built.
pub fn with_fee(fee: u64) -> Box<Append> {
	Box::new(move |_build, (tx, sum)| -> (Transaction, BlindSum) {
		(tx.with_fee(fee), sum)
	})
}

/// Sets a known excess value on the transaction being built. Usually used in
/// combination with the initial_tx function when a new transaction is built
/// by adding to a pre-existing one.
pub fn with_excess(excess: BlindingFactor) -> Box<Append> {
	Box::new(move |_build, (tx, sum)| -> (Transaction, BlindSum) {
		(tx, sum.add_blinding_factor(excess.clone()))
	})
}

/// Sets an initial transaction to add to when building a new transaction.
pub fn initial_tx(tx: Transaction) -> Box<Append> {
	Box::new(move |_build, (_, sum)| -> (Transaction, BlindSum) {
		(tx.clone(), sum)
	})
}

/// Builds a new transaction by combining all the combinators provided in a
/// Vector. Transactions can either be built "from scratch" with a list of
/// inputs or outputs or from a pre-existing transaction that gets added to.
///
/// Example:
/// let (tx1, sum) = build::transaction(vec![input_rand(4), output_rand(1),
///   with_fee(1)]).unwrap();
/// let (tx2, _) = build::transaction(vec![initial_tx(tx1), with_excess(sum),
///   output_rand(2)]).unwrap();
///
pub fn transaction(
	elems: Vec<Box<Append>>,
	keychain: &keychain::Keychain,
) -> Result<(Transaction, BlindingFactor), keychain::Error> {
	let mut ctx = Context { keychain };
	let (mut tx, sum) = elems.iter().fold(
		(Transaction::empty(), BlindSum::new()), |acc, elem| elem(&mut ctx, acc)
	);
	let blind_sum = ctx.keychain.blind_sum(&sum)?;
	let msg = secp::Message::from_slice(&u64_to_32bytes(tx.fee))?;
	let sig = ctx.keychain.sign_with_blinding(&msg, &blind_sum)?;
	tx.excess_sig = sig.serialize_der(&ctx.keychain.secp());
	Ok((tx, blind_sum))
}

fn u64_to_32bytes(n: u64) -> [u8; 32] {
	let mut bytes = [0; 32];
	BigEndian::write_u64(&mut bytes[24..32], n);
	bytes
}

// Just a simple test, most exhaustive tests in the core mod.rs.
#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn blind_simple_tx() {
		let keychain = Keychain::from_random_seed().unwrap();
		let pk1 = keychain.derive_pubkey(1).unwrap();
		let pk2 = keychain.derive_pubkey(2).unwrap();
		let pk3 = keychain.derive_pubkey(3).unwrap();

		let (tx, _) = transaction(
			vec![input(10, pk1), input(11, pk2), output(20, pk3), with_fee(1)],
			&keychain,
		).unwrap();

		tx.verify_sig(&keychain.secp()).unwrap();
	}

	#[test]
	fn blind_simpler_tx() {
		let keychain = Keychain::from_random_seed().unwrap();
		let pk1 = keychain.derive_pubkey(1).unwrap();
		let pk2 = keychain.derive_pubkey(2).unwrap();

		let (tx, _) = transaction(
			vec![input(6, pk1), output(2, pk2), with_fee(4)],
			&keychain,
		).unwrap();

		tx.verify_sig(&keychain.secp()).unwrap();
	}
}
