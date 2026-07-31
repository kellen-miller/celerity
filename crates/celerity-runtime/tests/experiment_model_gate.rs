use std::path::PathBuf;

use celerity_runtime::{
    CandidatePrediction, ExperimentAbort, ExperimentDecision, ExperimentPlan, InferenceBudget,
    ModelCommandSelection, ModelEligibilityInput, ModelIneligibility,
    select_model_or_deterministic,
};

#[test]
fn experiment_is_bounded_and_aborts_without_authority() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/experiments/duct-sweep.toml");
    let mut experiment = ExperimentPlan::load(&path).expect("plan").start(100);
    assert_eq!(
        experiment.advance(100, true, 90.0, true),
        ExperimentDecision::Apply {
            radiator_split_basis_points: 3000
        }
    );
    assert_eq!(
        experiment.advance(1100, true, 90.0, true),
        ExperimentDecision::Apply {
            radiator_split_basis_points: 5000
        }
    );
    assert_eq!(
        experiment.advance(1200, true, 90.0, false),
        ExperimentDecision::Abort(ExperimentAbort::AuthorityLost)
    );
    assert_eq!(
        experiment.advance(1300, true, 90.0, true),
        ExperimentDecision::Complete
    );
}

#[test]
fn model_gate_falls_back_for_budget_ood_and_no_feasible_candidate() {
    let base = ModelEligibilityInput {
        graph_present: true,
        compatible: true,
        history_complete: true,
        uncertainty: 0.1,
        maximum_uncertainty: 0.2,
        ood_score: 0.1,
        maximum_ood_score: 0.2,
        remaining_cycle_ns: 1_000,
    };
    let budget = InferenceBudget {
        measured_ceiling_ns: 500,
        required_post_work_ns: 200,
    };
    let candidates = [CandidatePrediction {
        split_basis_points: 6000,
        coolant_peak_c: 99.0,
        coolant_target_excess_c: 0.0,
        iat_cost: 2.0,
    }];
    assert_eq!(
        select_model_or_deterministic(base, budget, &candidates, 5000, 5000, 110.0),
        ModelCommandSelection::Optimized {
            split_basis_points: 6000
        }
    );
    assert_eq!(
        select_model_or_deterministic(
            ModelEligibilityInput {
                remaining_cycle_ns: 600,
                ..base
            },
            budget,
            &candidates,
            5000,
            5000,
            110.0,
        ),
        ModelCommandSelection::Deterministic {
            split_basis_points: 5000,
            reason: ModelIneligibility::InsufficientBudget,
        }
    );
    assert_eq!(
        select_model_or_deterministic(base, budget, &candidates, 5000, 5000, 98.0),
        ModelCommandSelection::Deterministic {
            split_basis_points: 5000,
            reason: ModelIneligibility::NoFeasibleCandidate,
        }
    );
}
