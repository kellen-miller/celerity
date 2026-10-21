use std::path::PathBuf;

use control_core::{
    CandidatePrediction, InferenceBudget, ModelError, OptimizerDecision, ThermalOptimizer,
    TractModel,
};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/models/identity.onnx")
}

#[test]
fn production_tract_wrapper_runs_real_onnx_fixture() {
    let model = TractModel::load(&fixture(), 3, 3).expect("load, warm, and benchmark fixture");
    assert_eq!(
        model.infer(&[95.0, 50.0, 0.9]).expect("finite inference"),
        vec![77.0, 59.0]
    );
    assert!(model.measured_ceiling_ns() > 0);
}

#[test]
fn corrupt_and_nonfinite_model_inputs_are_rejected() {
    assert!(matches!(
        TractModel::load(PathBuf::from("missing.onnx").as_path(), 3, 1),
        Err(ModelError::Load(_))
    ));
    let model = TractModel::load(&fixture(), 3, 1).expect("fixture");
    assert_eq!(
        model.infer(&[f32::NAN, 1.0, 0.5]),
        Err(ModelError::NonfiniteInput)
    );
}

#[test]
fn pre_inference_budget_gate_reserves_post_work() {
    let budget = InferenceBudget {
        measured_ceiling_ns: 80,
        required_post_work_ns: 20,
    };
    assert!(budget.allows(100));
    assert!(!budget.allows(99));
}

#[test]
fn optimizer_prioritizes_coolant_then_iat_then_movement() {
    let candidates = [
        CandidatePrediction {
            split_basis_points: 4_000,
            coolant_peak_c: 108.0,
            coolant_target_excess_c: 3.0,
            iat_cost: 1.0,
        },
        CandidatePrediction {
            split_basis_points: 6_000,
            coolant_peak_c: 104.0,
            coolant_target_excess_c: 1.0,
            iat_cost: 5.0,
        },
        CandidatePrediction {
            split_basis_points: 7_000,
            coolant_peak_c: 104.0,
            coolant_target_excess_c: 1.0,
            iat_cost: 4.0,
        },
        CandidatePrediction {
            split_basis_points: 8_000,
            coolant_peak_c: 104.0,
            coolant_target_excess_c: 1.0,
            iat_cost: 4.0,
        },
    ];
    assert_eq!(
        ThermalOptimizer::select(&candidates, 7_500, 105.0),
        OptimizerDecision::Selected {
            split_basis_points: 7_000
        }
    );
    assert_eq!(
        ThermalOptimizer::select(&candidates[..1], 5_000, 105.0),
        OptimizerDecision::NoFeasibleCandidate
    );
}
