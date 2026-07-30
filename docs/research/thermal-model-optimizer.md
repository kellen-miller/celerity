# Delayed thermal model and optimizer

## Decision

Use a **delay-aware, direct multi-horizon, multi-output ARX predictor**
with a small fixed operating-point basis, paired with
**finite-control-set robust model-predictive control (MPC)**.

The first promoted model should be a regularized multivariate regression, not
a recursive neural model:

- Inputs are a synchronized window of recent temperatures, vehicle operating
  signals, and commanded duct split, plus each candidate held split.
- Outputs are the stacked future coolant and post-intercooler charge-air
  temperature increments at fixed horizons.
- Explicit lag selection represents transport delay and thermal memory.
- A small, reviewable basis may schedule duct effectiveness by current load,
  speed, and ambient condition, while remaining linear in fitted parameters.
- An ensemble fitted by resampling whole experiments supplies epistemic
  disagreement. Held-out run residuals calibrate a one-sided, whole-trajectory
  coolant envelope.
- Range, local-support, covariance-distance, and ensemble-disagreement checks
  reject out-of-distribution (OOD) histories or candidate commands.

At each control cycle, the optimizer evaluates a bounded lattice of candidate
splits held across the prediction horizon. It discards every candidate whose
worst accepted coolant trajectory crosses the hard ceiling after uncertainty
inflation and an engineering margin. It then ranks feasible candidates
lexicographically:

1. deviation outside the human-configured coolant band;
2. charge-air temperature;
3. actuator movement.

Only the first command is applied, and the calculation repeats from fresh
measurements. If the model, inputs, timing, support checks, or feasibility
checks fail, model authority is revoked and the separate fallback controller
takes over.

This is a greenfield starting family, not a claim that the final fitted model
will be linear. The real data decides model order, delays, horizons, scheduling
terms, uncertainty widths, and whether a more nonlinear challenger earns
promotion.

## Why this family fits Celerity

### It models the quantity the controller actually needs

An ARX model represents current output from past outputs, past exogenous
inputs, and an explicit input-output delay. The standard MIMO form permits
different orders and delays for each input/output pair
([MathWorks ARX reference](https://www.mathworks.com/help/ident/ref/arx.html)).
Celerity should use those regressors to identify a **direct** predictor for
each useful future horizon rather than recursively rolling a one-step model
forward.

Direct multi-step predictors can be identified as linear regressions and used
directly in predictive-control optimization. In the linear stochastic setting,
they yield an MPC quadratic program equivalent to a state-space formulation
and permit simpler treatment of parameter uncertainty
([Köhler et al., 2023](https://arxiv.org/abs/2203.15471)). Direct prediction
also confines validation to the exact horizons Celerity uses; it does not
require trusting an unvalidated long recursive rollout.

The model should predict temperature **increments from the current measured
temperatures**. That anchors each forecast to current reality and makes
coolant and charge-air dynamics share the same history and candidate command
without pretending that their delays or time constants are equal.

### It keeps delay and operating dependence visible

For each forecast origin \(t\), construct a history vector
\(\phi_t\) containing:

- coolant and charge-air temperatures;
- engine speed and load proxies available from the receive-only vehicle bus;
- vehicle speed and available ambient/intake conditions;
- fan, pump, or other thermal-state signals if they are actually available;
- past accepted duct-split commands;
- time derivatives or short-window summaries only when replay shows that raw
  lags are insufficient.

For a held candidate split \(u\), predict the stacked vector

\[
\Delta\hat{Y}_{t} =
\begin{bmatrix}
\Delta\hat{T}_{cool,t+h_1} \\
\Delta\hat{T}_{charge,t+h_1} \\
\vdots \\
\Delta\hat{T}_{cool,t+h_H} \\
\Delta\hat{T}_{charge,t+h_H}
\end{bmatrix}
= W^\top
\begin{bmatrix}
1 \\
\phi_t \\
b(u) \\
s(\phi_t) \otimes b(u)
\end{bmatrix}.
\]

Here \(b(u)\) is a deliberately small split basis and \(s(\phi_t)\) contains a
small set of predeclared scheduling terms such as current load, speed, and
ambient bins or splines. The interaction allows duct effectiveness to vary
with airflow and heat load while fitting one linear-in-parameters model.
Regularization controls variance when the lag bank and interactions are
correlated; ARX estimation explicitly supports regularization
([MathWorks ARX reference](https://www.mathworks.com/help/ident/ref/arx.html)).

Start with \(b(u)=u\). Add a hinge or quadratic split term, or a small number
of scheduling interactions, only if grouped validation shows a repeatable
error pattern. Do not start with an unrestricted nonlinear regressor bank.

The candidate model set should include simple first- and second-order
dead-time process models as interpretability baselines. Such models expose
gain, time constant, and transport delay directly
([MathWorks process-model documentation](https://www.mathworks.com/help/ident/process-models.html)).
They are useful diagnostics even if the joint predictor ultimately wins.

### It provides confidence without pretending confidence is safety

Fit \(B\) model members by resampling **whole experiment runs**, not adjacent
rows. For every candidate and horizon, retain the member predictions rather
than only their mean. Calibrate an additional one-sided coolant residual on
held-out runs using a score based on the maximum underprediction across the
entire coolant trajectory. This produces one trajectory-level inflation value
instead of incorrectly treating horizon violations as independent.

Bootstrap-ensemble prediction intervals can be adapted to dependent time
series, but their finite-sample coverage is only approximate under stated
mixing assumptions
([Xu and Xie, 2021](https://proceedings.mlr.press/v139/xu21h.html)).
Multi-horizon conformal methods can target joint trajectory coverage
([Galvão Lopes et al., 2024](https://proceedings.mlr.press/v230/galvao-lopes24a.html)).
Those results justify calibration as a useful confidence mechanism, not as a
physical guarantee for this vehicle.

For candidate \(u\) and coolant horizon \(h\), use a bound shaped like

\[
U_{cool,h}(u) =
\max_{b \in 1..B}
\hat{T}^{(b)}_{cool,h}(u)
+ q_{cool,h},
\]

or a joint-trajectory variant using one calibrated maximum-error score. A
candidate is model-feasible only if every \(U_{cool,h}(u)\) is below the
configured hard ceiling minus an engineering margin.

The hard ceiling cannot be guaranteed from empirical coverage alone.
Robust MPC safety results require a valid bounded uncertainty set and safe
terminal or fallback behavior; model-predictive safety certification achieves
constraint guarantees by combining robust prediction with a known safe set
and backup policy
([Wabersich and Zeilinger, 2018](https://arxiv.org/abs/1803.08552)).
Celerity therefore needs an independent measured-temperature limit,
staleness/health checks, and a physically characterized fallback regardless
of model metrics.

### OOD rejection is an authority decision

Reject model control before optimization when:

- any required signal is missing, stale, invalid, or outside the artifact's
  admitted physical/training range;
- the complete lagged history lacks local training support in the current
  operating regime;
- a robust covariance-distance score exceeds its validated threshold;
- ensemble trajectory spread exceeds its validated threshold.

Also reject individual split candidates that lack command support near the
current operating regime. A global 10%-to-90% command range is not sufficient
evidence that every split has been observed under every heat-load and airflow
condition.

Hotelling's \(T^2\) accounts for multivariate covariance when measuring
distance from a reference population
([NIST handbook](https://www.itl.nist.gov/div898/handbook/pmc/section3/pmc341.htm)).
It is only one OOD feature here: a single elliptical distance can miss holes
in training support, so Celerity must combine it with range checks, local
support counts, and ensemble disagreement. Thresholds are acceptance-test
parameters, not generic statistical defaults.

## Optimizer shape

Use **finite-control-set robust MPC with one held move** initially:

1. Build the candidate split lattice within the installed actuator's allowed
   range and rate limit.
2. Evaluate all model-ensemble trajectories for each candidate.
3. Reject candidates outside model support.
4. Reject candidates whose inflated coolant trajectory violates the hard
   ceiling margin.
5. Rank the remaining candidates by the ordered coolant-band, charge-air, and
   movement objectives.
6. Apply the best split and repeat at the next control instant.

Here, "robust" means that a candidate must satisfy the constraint for every
member of the admitted empirical uncertainty envelope. It does not claim a
plant-level robust invariant set; that would require validated physical
disturbance bounds and safe terminal behavior that Celerity does not yet have.

This is MPC even though the first solver is enumeration: it optimizes predicted
future trajectories, applies only the first move, and replans. For one slow
actuator, bounded enumeration has attractive properties:

- worst-case runtime is known from candidate count, ensemble size, and horizon
  count;
- every constraint and rejection reason is inspectable;
- nonlinear split basis terms do not introduce local optimizer minima;
- infeasibility is explicit;
- the same code can score every candidate during offline replay.

Do not choose the command-lattice spacing yet. It must follow measured
actuator resolution, useful thermal sensitivity, and Raspberry Pi timing.
Likewise, start with a single held move because the requested predictor takes
a candidate commanded split. Add one future move block only if replay and
shadow operation show material benefit that survives the expanded
counterfactual uncertainty.

If later evidence shows that a continuous command or several move blocks
matter, the same direct affine predictor and quadratic soft costs can be
compiled into a constrained convex quadratic program. Standard linear MPC
repeatedly solves a finite-horizon constrained QP
([OSQP MPC example](https://osqp.org/docs/examples/mpc.html)), and OSQP can
generate fixed-dimension embedded C code
([OSQP code-generation documentation](https://osqp.org/docs/codegen/index.html)).
That is the planned optimizer escalation, not the first-release default.

## Objective and authority semantics

The model and optimizer artifact must preserve this order:

1. **Runtime authority gate:** model compatibility, data validity, controller
   heartbeat, OOD acceptance, and computation deadline.
2. **Hard feasibility gate:** inflated coolant trajectory below
   `hard_ceiling - engineering_margin`, plus actuator range/rate/dwell limits.
3. **Preferred coolant band:** minimize predicted excursion outside the
   configured lower and upper band.
4. **Charge-air objective:** among coolant-equivalent candidates, prefer lower
   post-intercooler charge-air temperature.
5. **Movement objective:** among thermally equivalent candidates, prefer less
   command movement and respect minimum dwell.

The runtime must log the candidate set, rejection reason for each candidate,
chosen candidate, predicted trajectories, confidence envelope, OOD scores,
and whether authority came from the model or fallback. If no candidate is
model-feasible, the optimizer must not invent a least-bad model action; it
must revoke authority and enter the separately decided fallback behavior.

## Data and experiment requirements

### Acquisition

- Use the Pi-owned synchronized log as the only training input. Model fitting
  requires uniformly sampled inputs and outputs recorded at the same instants
  ([MathWorks `iddata` documentation](https://www.mathworks.com/help/ident/ref/iddata.html)).
- Preserve raw timestamps, source timestamps if available, receive age,
  validity flags, units, command issue time, Nano acceptance, and the
  fallback/model authority state.
- Treat run boundaries, key cycles, software versions, sensor calibration,
  and hardware configuration as first-class grouping metadata.
- Never silently interpolate a gap that runtime would classify as stale.
  Training preprocessing and runtime preprocessing must share one versioned
  implementation.

### Excitation

The planned repeated, varied step-and-hold experiments are suitable only if
they excite the dynamics across safe operating conditions. Identification
accuracy depends on experiment design
([MathWorks system-identification overview](https://www.mathworks.com/help/ident/gs/about-system-identification.html)),
and identification inputs should cover the dynamic bandwidth of interest
([MathWorks `idinput` documentation](https://www.mathworks.com/help/ident/ref/idinput.html)).

Within safe operator and thermal limits:

- repeat every tested split in both movement directions;
- vary starting coolant and charge-air temperatures;
- vary hold duration so delay and time constants are distinguishable;
- include no-move baselines;
- cover relevant combinations of engine load, speed, vehicle speed, and
  ambient condition rather than collecting most samples in easy cruise;
- randomize or counterbalance the safe split order so command is not merely a
  proxy for time, warming trend, or driver behavior;
- repeat comparable conditions on separate runs and days.

Because the first release has no independent position sensor, the model input
is accepted **commanded** position. The fitted uncertainty must therefore
include Nano/actuator tracking variation. If this uncertainty makes the hard
constraint envelope unusably wide, the correct result is to withhold model
authority or add position feedback, not to narrow the interval.

### Partitioning and metrics

Split train, tuning, calibration, and final test sets by complete run or day;
never randomly split adjacent rows. Keep a final condition-stratified test set
untouched until promotion review.

At minimum, report by output, horizon, and operating regime:

- mean absolute and root-mean-square trajectory error;
- coolant underprediction rate, magnitude, and worst case;
- joint coolant-envelope coverage and width;
- charge-air prediction error;
- OOD false-accept and false-reject behavior on deliberately withheld
  conditions and corrupted-input cases;
- candidate-feasibility and fallback rates;
- end-to-end worst-case inference time and deadline misses on the target Pi.

Validate at the horizons that represent the system's dominant time constants,
as recommended for control-oriented identified models
([MathWorks simulation and prediction documentation](https://www.mathworks.com/help/ident/ug/definition-simulation-and-prediction.html)).
The useful horizons themselves must come from the experiments.

Offline control replay can reproduce what the runtime would have predicted and
selected from each logged history. It cannot provide factual temperatures for
a split that was not actually commanded. Counterfactual replay is therefore
model evidence, while held-out prediction, shadow operation, and later
controlled trials provide plant evidence.

## Promotion and rollback

Package the following as one immutable, content-addressed artifact:

- feature names, units, ordering, sample period, lag window, and preprocessing
  version;
- horizon list and output ordering;
- coefficient matrices and basis definition;
- training-run manifest and code/data hashes;
- ensemble members and residual-envelope calibration;
- admitted ranges, local-support representation, covariance parameters, and
  OOD thresholds;
- command lattice, movement constraints, objective ordering, and engineering
  margin;
- validation report and target-runtime compatibility version.

Promotion is explicit and reversible:

1. Train candidates offline without changing the active artifact.
2. Replay the candidate and current active artifact on the same frozen suites.
3. Reject any candidate with worse coolant underprediction, inadequate
   envelope coverage, unacceptable OOD behavior, or missed Pi deadlines,
   regardless of average temperature error.
4. Run the candidate live in shadow mode, logging proposals without actuator
   authority.
5. Promote by atomically changing the active artifact pointer after human
   approval.
6. Retain the previous known-good artifact and its validation evidence.
7. Revoke to fallback immediately on runtime health/OOD/deadline failure; allow
   an operator to atomically roll the pointer back without retraining.

No first-release artifact learns online. New logs may train a new immutable
candidate, but activation always follows replay, shadow evidence, and explicit
promotion.

## Alternatives considered

| Family | Strength | Why it is not the first promoted family |
| --- | --- | --- |
| Low-order grey-box RC/FOPDT or state-space | Data-efficient, inspectable delays and time constants | Keep as mandatory baselines. With unknown airflow/heat rejection and commanded rather than measured flap position, a single fixed model may confound operating regime with duct effect. Promote it if data shows it is sufficient. |
| Subspace state-space identification | N4SID recovers multivariable state-space models from input/output projections using non-iterative QR/SVD methods ([Van Overschee and De Moor, 1994](https://doi.org/10.1016/0005-1098%2894%2990230-5)) | A latent LTI realization is attractive, but Celerity only needs bounded-horizon output prediction and must handle strong operating-point dependence and explicit support checks. Use it as a challenger after the delay-aware direct baseline. |
| Gaussian-process NARX or GP state-space model | Flexible nonlinear fit with native posterior uncertainty; cautious GP-MPC explicitly propagates approximate uncertainty into chance constraints ([Hewing et al., 2019](https://arxiv.org/abs/1705.10702)) | Multi-output exact GP inference scales cubically in sample count and output count ([Bruinsma et al., 2020](https://proceedings.mlr.press/v119/bruinsma20a.html)); sparse and multi-step approximations add validation choices. Consider a sparse GP **residual** challenger only if structured replay shows repeatable nonlinear residuals. |
| Koopman lifted linear predictor | Nonlinear lifting can retain a linear-MPC optimization shape ([Korda and Mezić, 2018](https://arxiv.org/abs/1611.03537)) | Dictionary/lift selection is an additional learned boundary, while calibrated uncertainty and OOD rejection still need separate mechanisms. It has no demonstrated advantage here before data exists. |
| SINDYc | Sparse regression can produce interpretable controlled nonlinear equations ([Brunton et al., 2016](https://arxiv.org/abs/1605.06682)) | It assumes useful dynamics are sparse in a chosen function library. Hidden thermal states, delays, and unmeasured actuator position make that assumption premature; uncertainty is not native. Revisit if measured data reveals a compact physical basis. |
| DeePC | For deterministic LTI systems, DeePC directly uses trajectory data and is equivalent to classical MPC; noisy/nonlinear cases require regularization ([Coulson et al., 2019](https://arxiv.org/abs/1811.05890)) | Its trajectory matrices and noise regularization move model complexity into the online controller. OOD, calibrated multi-horizon confidence, and an easily inspectable Pi artifact are more direct with the selected predictor. |
| Neural state-space, RNN, or temporal convolution model | Can represent complex nonlinear memory; neural constrained policies have run on Raspberry Pi hardware ([Drgoňa et al., 2020](https://arxiv.org/abs/2011.03699)) | Pi execution is not the objection. Mission-critical neural state-space uncertainty remains an active approximate-inference problem ([Forgione and Piga, 2023](https://arxiv.org/abs/2304.06349)). The added representation and calibration burden is unjustified until simpler grouped replay fails. |
| Direct sensor-to-servo policy or reinforcement learning | Potentially low online compute | It fuses prediction, prioritization, constraints, and actuator choice, preventing independent trajectory validation and transparent counterfactual ranking. It is incompatible with the required hard feasibility gate and explicit model promotion evidence. |
| Nonlinear or chance-constrained MPC as the default | Can optimize a richer nonlinear model and probabilistic constraints | It adds solver and approximation failure modes before data establishes a need. Chance constraints accept a nonzero violation probability; the coolant ceiling instead requires a separate hard authority/fallback boundary. |

## What is decided now

- Model control remains separate from safety/fallback authority.
- The first model family is a regularized, direct multi-horizon MIMO ARX
  predictor with explicit delays and only small predeclared scheduling terms.
- Confidence uses run-resampled ensembles plus held-out one-sided trajectory
  residual calibration.
- OOD rejection combines boundary, local-support, covariance-distance, and
  ensemble-disagreement checks.
- The first optimizer is finite-control-set robust MPC over one held split,
  with the hard coolant gate ahead of lexicographically ordered soft goals.
- Training is offline; artifacts are immutable; promotion is explicit; rollback
  is an atomic pointer change; runtime failures revoke to fallback.
- Grey-box dead-time models are mandatory baselines. GP residual, subspace,
  sparse nonlinear, Koopman, DeePC, and neural models are challengers, not
  defaults.

## What must await real data

- synchronized sample period and preprocessing/filtering;
- useful forecast horizons for coolant and charge air;
- lag-window length, per-output command delay, and effective actuator lag;
- which vehicle signals are both available and predictively necessary;
- whether one global model is adequate or which scheduling terms/regimes are
  repeatable;
- linear, hinge, or other small split basis;
- regularization strength, ensemble size, and residual calibration method;
- command-lattice spacing, rate limit, dwell time, and whether a future move
  block earns its complexity;
- engineering margin, OOD thresholds, and acceptable confidence-envelope
  width;
- worst-case Pi runtime and memory budget on the exact hardware/software stack;
- whether commanded-position uncertainty is tolerable without physical
  position feedback;
- whether any nonlinear challenger improves held-out safety metrics enough to
  justify promotion.

## Unresolved validation questions

1. Does the commanded split have a repeatable causal effect on both outputs
   after controlling for load, airflow, ambient condition, and warming trend?
2. Are coolant and charge-air delays stable enough for one artifact, or do they
   shift materially by operating regime?
3. What horizon is long enough to expose an unsafe coolant trend while still
   short enough that unknown future driver demand does not dominate?
4. Can a run-calibrated one-sided envelope achieve the required
   underprediction rate without rejecting most useful candidates?
5. Which deliberately withheld days, temperatures, speeds, loads, and sensor
   corruptions constitute an adequate OOD acceptance suite?
6. Does a held single-move search outperform the fallback in shadow/replay
   evidence, and does one future move add enough value to warrant a larger
   counterfactual surface?
7. What physical fallback action and engineering margin make the hard ceiling
   defensible when model predictions, actuator tracking, or future heat load
   are wrong?
8. How quickly must the controller revoke model authority as measured coolant
   approaches the ceiling, independent of the predicted trajectory?
9. What maximum model-envelope width, fallback rate, and computation deadline
   miss rate are acceptable for promotion?

Until these questions have data-backed answers, Celerity has selected a model
and optimizer **family**, not a fitted controller or a deployment-ready safety
case.
