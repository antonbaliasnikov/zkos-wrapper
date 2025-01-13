use crate::verifier::transcript::*;
use boojum::{
    blake2::*, config::CSConfig, cs::{gates::{
        ConstantsAllocatorGate, ReductionGate, U32TriAddCarryAsChunkGate
    }, traits::{cs::ConstraintSystem, gate::GatePlacementStrategy}, CSGeometry}, dag::CircuitResolverOpts, gadgets::{blake2s::blake2s, tables::{
        byte_split::{create_byte_split_table, ByteSplitTable},
        xor8::{create_xor8_table, Xor8Table},
    }, traits::witnessable::WitnessHookable, u32::UInt32, u8::UInt8},
    gadgets::blake2s::mixing_function::Word,
    gadgets::traits::allocatable::CSAllocatable,
    gadgets::blake2s::round_function::Blake2sControl,
};
use std::alloc::Global;
use crate::verifier::Blake2sStateGate;
use crate::verifier::blake2s_reduced_round_function;
use zkos_verifier::{blake2s_u32::CONFIGURED_IV, prover::cs::cs::circuit};
use std::mem::MaybeUninit;

type F = boojum::field::goldilocks::GoldilocksField;


// #[test]
// fn test_transcript_circuit() {
//     let input_len = 64;
//     let mut input_u32: Vec<u32> = vec![];
//     for i in 0..input_len {
//         input_u32.push(i as u32);
//     }


    
// }

// #[test]
// fn test_single_round_exact() {
//     test_blake2s(64);
// }

#[test]
fn test_blake2s_round_function() {
    let len = 16;
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);

    let mut input = vec![];
    for _ in 0..len {
        let byte: u32 = rng.r#gen();
        input.push(byte);
    }

    let mut hasher = zkos_verifier::blake2s_u32::Blake2sState::new();
    hasher.input_buffer.iter_mut().enumerate().for_each(|(i, x)| {
        *x = input[i];
    });
    unsafe { hasher.run_round_function::<true>(len, true); }
    let reference_output = hasher.read_state_for_output();

    let geometry = CSGeometry {
        num_columns_under_copy_permutation: 20,
        num_witness_columns: 0,
        num_constant_columns: 4,
        max_allowed_constraint_degree: 4,
    };

    use boojum::config::DevCSConfig;
    type RCfg = <DevCSConfig as CSConfig>::ResolverConfig;
    use boojum::cs::cs_builder_reference::*;
    let builder_impl =
        CsReferenceImplementationBuilder::<F, F, DevCSConfig>::new(geometry, 1 << 17);
    use boojum::cs::cs_builder::new_builder;
    let builder = new_builder::<_, F>(builder_impl);

    let builder = builder.allow_lookup(
        boojum::cs::LookupParameters::UseSpecializedColumnsWithTableIdAsConstant {
            width: 3,
            num_repetitions: 5,
            share_table_id: true,
        },
    );
    let builder = ConstantsAllocatorGate::configure_builder(
        builder,
        GatePlacementStrategy::UseGeneralPurposeColumns,
    );
    let builder = U32TriAddCarryAsChunkGate::configure_builder(
        builder,
        GatePlacementStrategy::UseGeneralPurposeColumns,
    );
    let builder = ReductionGate::<F, 4>::configure_builder(
        builder,
        GatePlacementStrategy::UseGeneralPurposeColumns,
    );

    let mut owned_cs = builder.build(CircuitResolverOpts::new(1 << 20));

    // add tables
    let table = create_xor8_table();
    owned_cs.add_lookup_table::<Xor8Table, 3>(table);

    let table = create_byte_split_table::<F, 4>();
    owned_cs.add_lookup_table::<ByteSplitTable<4>, 3>(table);

    let table = create_byte_split_table::<F, 7>();
    owned_cs.add_lookup_table::<ByteSplitTable<7>, 3>(table);

    let table = create_byte_split_table::<F, 1>();
    owned_cs.add_lookup_table::<ByteSplitTable<1>, 3>(table);

    // let table = create_byte_split_table::<F, 2>();
    // owned_cs.add_lookup_table::<ByteSplitTable<2>, 3>(table);

    // let table = create_byte_split_table::<F, 3>();
    // owned_cs.add_lookup_table::<ByteSplitTable<3>, 3>(table);

    let mut circuit_input = vec![];

    let cs = &mut owned_cs;

    for val in input.iter() {
        let val = UInt32::allocate_checked(cs, *val);
        circuit_input.push(val);
    }

    let mut hasher = Blake2sStateGate::new(cs);
    hasher.input_buffer.iter_mut().enumerate().for_each(|(i, x)| {
        *x = Word { inner: circuit_input[i].to_le_bytes(cs) };
    });
    unsafe { hasher.run_round_function::<_, true>(cs, len, true); }
    let output = hasher
        .read_state_for_output()
        .map(|el| UInt32::from_le_bytes(cs, el.inner));

    let output = output.witness_hook(cs)().unwrap();
    let reference_output = reference_output.as_slice();
    assert_eq!(output, reference_output);

    drop(cs);
    let _owned_cs = owned_cs.into_assembly::<Global>();
}


#[test]
fn test_transcript_circuit_initial() {
    test_transcript_circuit(200);
}

fn test_transcript_circuit(len: usize) {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);

    let mut input = vec![];
    for i in 0..len {
        let byte: u32 = i as u32;// rng.r#gen();
        input.push(byte);
    }

    // let pow_nonce: u64 = zkos_verifier::field::Mersenne31Field::ORDER as u64; //192720620;// rng.r#gen();
    const POW_BITS: u32 = 28;

    let mut transcript_hasher = zkos_verifier::blake2s_u32::Blake2sState::new();
    let mut seed = zkos_verifier::transcript::Blake2sTranscript::commit_initial_using_hasher(
        &mut transcript_hasher,
        &input[..],
    );
    zkos_verifier::transcript::Blake2sTranscript::commit_with_seed_using_hasher(
        &mut transcript_hasher, 
        &mut seed, 
        &input
    );
    let mut transcript_challenges = unsafe {
        MaybeUninit::<
            [u32; (1usize * 4).next_multiple_of(zkos_verifier::blake2s_u32::BLAKE2S_DIGEST_SIZE_U32_WORDS)],
        >::uninit()
        .assume_init()
    };
    zkos_verifier::transcript::Blake2sTranscript::draw_randomness_using_hasher(
        &mut transcript_hasher,
        &mut seed,
        &mut transcript_challenges,
    );

    let worker = zkos_verifier_worker::Worker::new_with_num_threads(8);
    let (mut seed, pow_nonce) = zkos_verifier::transcript::Blake2sTranscript::search_pow(&seed, POW_BITS, &worker);
    let mut transcript_hasher = zkos_verifier::blake2s_u32::Blake2sState::new();
    zkos_verifier::transcript::Blake2sTranscript::verify_pow_using_hasher(
        &mut transcript_hasher,
        &mut seed,
        pow_nonce as u64,
        POW_BITS as u32,
    );
    
    let reference_output = transcript_challenges;// seed.0;

    let geometry = CSGeometry {
        num_columns_under_copy_permutation: 20,
        num_witness_columns: 0,
        num_constant_columns: 4,
        max_allowed_constraint_degree: 4,
    };

    use boojum::config::DevCSConfig;
    type RCfg = <DevCSConfig as CSConfig>::ResolverConfig;
    use boojum::cs::cs_builder_reference::*;
    let builder_impl =
        CsReferenceImplementationBuilder::<F, F, DevCSConfig>::new(geometry, 1 << 17);
    use boojum::cs::cs_builder::new_builder;
    let builder = new_builder::<_, F>(builder_impl);

    let builder = builder.allow_lookup(
        boojum::cs::LookupParameters::UseSpecializedColumnsWithTableIdAsConstant {
            width: 3,
            num_repetitions: 5,
            share_table_id: true,
        },
    );
    let builder = ConstantsAllocatorGate::configure_builder(
        builder,
        GatePlacementStrategy::UseGeneralPurposeColumns,
    );
    let builder = U32TriAddCarryAsChunkGate::configure_builder(
        builder,
        GatePlacementStrategy::UseGeneralPurposeColumns,
    );
    let builder = ReductionGate::<F, 4>::configure_builder(
        builder,
        GatePlacementStrategy::UseGeneralPurposeColumns,
    );

    let mut owned_cs = builder.build(CircuitResolverOpts::new(1 << 20));

    // add tables
    let table = create_xor8_table();
    owned_cs.add_lookup_table::<Xor8Table, 3>(table);

    let table = create_byte_split_table::<F, 4>();
    owned_cs.add_lookup_table::<ByteSplitTable<4>, 3>(table);

    let table = create_byte_split_table::<F, 7>();
    owned_cs.add_lookup_table::<ByteSplitTable<7>, 3>(table);

    let table = create_byte_split_table::<F, 1>();
    owned_cs.add_lookup_table::<ByteSplitTable<1>, 3>(table);

    // let table = create_byte_split_table::<F, 2>();
    // owned_cs.add_lookup_table::<ByteSplitTable<2>, 3>(table);

    // let table = create_byte_split_table::<F, 3>();
    // owned_cs.add_lookup_table::<ByteSplitTable<3>, 3>(table);

    let mut circuit_input = vec![];

    let cs = &mut owned_cs;

    for pair in input.iter() {
        let pair = UInt32::<F>::allocate_checked(cs, *pair);
        circuit_input.push(pair);
    }

    let pow_nonce = [UInt32::allocate_checked(cs, pow_nonce as u32), UInt32::zero(cs)];

    // let output = blake2s(cs, &circuit_input);
    let mut transcript_hasher = Blake2sStateGate::new(cs);
    let mut seed = Blake2sWrappedTranscript::commit_initial_using_hasher(
        cs,
        &mut transcript_hasher,
        &circuit_input,
    );
    Blake2sWrappedTranscript::commit_with_seed_using_hasher(
        cs,
        &mut transcript_hasher,
        &mut seed,
        &circuit_input,
    );
    let mut transcript_challenges = 
        [UInt32::zero(cs); (1usize * 4).next_multiple_of(zkos_verifier::blake2s_u32::BLAKE2S_DIGEST_SIZE_U32_WORDS)];
    Blake2sWrappedTranscript::draw_randomness_using_hasher(
        cs,
        &mut transcript_hasher,
        &mut seed,
        &mut transcript_challenges,
    );
    Blake2sWrappedTranscript::verify_pow_using_hasher::<_, _, POW_BITS>(
        cs,
        &mut transcript_hasher,
        &mut seed,
        pow_nonce,
        // POW_BITS as u32,
    );
    let output = transcript_challenges;// seed.0.map(|el| UInt32::from_le_bytes(cs, el.inner));
    
    let output = output.witness_hook(cs)().unwrap();
    let reference_output = reference_output.as_slice();
    assert_eq!(output, reference_output);

    drop(cs);
    let _owned_cs = owned_cs.into_assembly::<Global>();
}


#[test]
fn test_decompose() {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);

    let input: u32 = rng.r#gen();

    let reference_output: [u8; 4] = std::array::from_fn(|idx| {
        (input >> (idx * 8)) as u8
    });
    let reference_output = input;

    let geometry = CSGeometry {
        num_columns_under_copy_permutation: 20,
        num_witness_columns: 0,
        num_constant_columns: 4,
        max_allowed_constraint_degree: 4,
    };

    use boojum::config::DevCSConfig;
    type RCfg = <DevCSConfig as CSConfig>::ResolverConfig;
    use boojum::cs::cs_builder_reference::*;
    let builder_impl =
        CsReferenceImplementationBuilder::<F, F, DevCSConfig>::new(geometry, 1 << 17);
    use boojum::cs::cs_builder::new_builder;
    let builder = new_builder::<_, F>(builder_impl);

    let builder = builder.allow_lookup(
        boojum::cs::LookupParameters::UseSpecializedColumnsWithTableIdAsConstant {
            width: 3,
            num_repetitions: 5,
            share_table_id: true,
        },
    );
    let builder = ConstantsAllocatorGate::configure_builder(
        builder,
        GatePlacementStrategy::UseGeneralPurposeColumns,
    );
    let builder = U32TriAddCarryAsChunkGate::configure_builder(
        builder,
        GatePlacementStrategy::UseGeneralPurposeColumns,
    );
    let builder = ReductionGate::<F, 4>::configure_builder(
        builder,
        GatePlacementStrategy::UseGeneralPurposeColumns,
    );

    let mut owned_cs = builder.build(CircuitResolverOpts::new(1 << 20));

    // add tables
    let table = create_xor8_table();
    owned_cs.add_lookup_table::<Xor8Table, 3>(table);

    let table = create_byte_split_table::<F, 4>();
    owned_cs.add_lookup_table::<ByteSplitTable<4>, 3>(table);

    let table = create_byte_split_table::<F, 7>();
    owned_cs.add_lookup_table::<ByteSplitTable<7>, 3>(table);

    let table = create_byte_split_table::<F, 1>();
    owned_cs.add_lookup_table::<ByteSplitTable<1>, 3>(table);

    let table = create_byte_split_table::<F, 2>();
    owned_cs.add_lookup_table::<ByteSplitTable<2>, 3>(table);

    let table = create_byte_split_table::<F, 3>();
    owned_cs.add_lookup_table::<ByteSplitTable<3>, 3>(table);

    let cs = &mut owned_cs;

    let circuit_input = UInt32::<F>::allocate_checked(cs, input);

    let output = circuit_input.to_le_bytes(cs);
    let output = UInt32::from_le_bytes(cs, output);
    
    let output = output.witness_hook(cs)().unwrap();
    let reference_output = reference_output;//.as_slice();
    assert_eq!(output, reference_output);

    drop(cs);
    let _owned_cs = owned_cs.into_assembly::<Global>();
}