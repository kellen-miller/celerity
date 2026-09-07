from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class Direction(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    DIRECTION_UNSPECIFIED: _ClassVar[Direction]
    DIRECTION_RECEIVE: _ClassVar[Direction]
    DIRECTION_TRANSMIT: _ClassVar[Direction]

class Quality(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    QUALITY_UNSPECIFIED: _ClassVar[Quality]
    QUALITY_VALID: _ClassVar[Quality]
    QUALITY_INVALID_VALUE: _ClassVar[Quality]
    QUALITY_UNKNOWN: _ClassVar[Quality]

class Completion(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    COMPLETION_UNSPECIFIED: _ClassVar[Completion]
    COMPLETION_COMPLETE: _ClassVar[Completion]
    COMPLETION_INCOMPLETE: _ClassVar[Completion]

class JobState(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    JOB_STATE_UNSPECIFIED: _ClassVar[JobState]
    JOB_STATE_QUEUED: _ClassVar[JobState]
    JOB_STATE_RUNNING: _ClassVar[JobState]
    JOB_STATE_COMPLETED: _ClassVar[JobState]
    JOB_STATE_NO_CHANGE: _ClassVar[JobState]
    JOB_STATE_REJECTED: _ClassVar[JobState]
    JOB_STATE_FAILED: _ClassVar[JobState]

class WebhookEventType(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    WEBHOOK_EVENT_TYPE_UNSPECIFIED: _ClassVar[WebhookEventType]
    WEBHOOK_EVENT_TYPE_TRAINING_FAILURE: _ClassVar[WebhookEventType]
    WEBHOOK_EVENT_TYPE_CANDIDATE_REJECTION: _ClassVar[WebhookEventType]
    WEBHOOK_EVENT_TYPE_DEMOTION: _ClassVar[WebhookEventType]
    WEBHOOK_EVENT_TYPE_ROLLBACK: _ClassVar[WebhookEventType]

class GlobalAuthority(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    GLOBAL_AUTHORITY_UNSPECIFIED: _ClassVar[GlobalAuthority]
    GLOBAL_AUTHORITY_FALLBACK: _ClassVar[GlobalAuthority]
    GLOBAL_AUTHORITY_ACTIVE: _ClassVar[GlobalAuthority]
    GLOBAL_AUTHORITY_HARD_FAULT: _ClassVar[GlobalAuthority]

class FeatureAuthority(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    FEATURE_AUTHORITY_UNSPECIFIED: _ClassVar[FeatureAuthority]
    FEATURE_AUTHORITY_FALLBACK: _ClassVar[FeatureAuthority]
    FEATURE_AUTHORITY_ARMING: _ClassVar[FeatureAuthority]
    FEATURE_AUTHORITY_ACTIVE: _ClassVar[FeatureAuthority]

class CommandSource(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    COMMAND_SOURCE_UNSPECIFIED: _ClassVar[CommandSource]
    COMMAND_SOURCE_CONTROLLER_LOCAL_FALLBACK: _ClassVar[CommandSource]
    COMMAND_SOURCE_DETERMINISTIC: _ClassVar[CommandSource]
    COMMAND_SOURCE_MODEL_OPTIMIZED: _ClassVar[CommandSource]
    COMMAND_SOURCE_EXPERIMENT: _ClassVar[CommandSource]

class DiagnosticHealthState(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    DIAGNOSTIC_HEALTH_STATE_UNSPECIFIED: _ClassVar[DiagnosticHealthState]
    DIAGNOSTIC_HEALTH_STATE_UNKNOWN: _ClassVar[DiagnosticHealthState]
    DIAGNOSTIC_HEALTH_STATE_HEALTHY: _ClassVar[DiagnosticHealthState]
    DIAGNOSTIC_HEALTH_STATE_UNHEALTHY: _ClassVar[DiagnosticHealthState]

class DiagnosticUnknownReason(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    DIAGNOSTIC_UNKNOWN_REASON_UNSPECIFIED: _ClassVar[DiagnosticUnknownReason]
    DIAGNOSTIC_UNKNOWN_REASON_NEVER_OBSERVED: _ClassVar[DiagnosticUnknownReason]
    DIAGNOSTIC_UNKNOWN_REASON_NOT_EXPECTED_IN_FALLBACK: _ClassVar[DiagnosticUnknownReason]
    DIAGNOSTIC_UNKNOWN_REASON_NOT_RENEWING: _ClassVar[DiagnosticUnknownReason]
    DIAGNOSTIC_UNKNOWN_REASON_NO_OUTSTANDING_COMMAND: _ClassVar[DiagnosticUnknownReason]
    DIAGNOSTIC_UNKNOWN_REASON_AWAITING_EVIDENCE: _ClassVar[DiagnosticUnknownReason]

class RunStorageHealth(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    RUN_STORAGE_HEALTH_UNSPECIFIED: _ClassVar[RunStorageHealth]
    RUN_STORAGE_HEALTH_UNKNOWN: _ClassVar[RunStorageHealth]
    RUN_STORAGE_HEALTH_HEALTHY: _ClassVar[RunStorageHealth]
    RUN_STORAGE_HEALTH_DEGRADED: _ClassVar[RunStorageHealth]

class PresentationState(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    PRESENTATION_STATE_UNSPECIFIED: _ClassVar[PresentationState]
    PRESENTATION_STATE_CURRENT: _ClassVar[PresentationState]
    PRESENTATION_STATE_STALE: _ClassVar[PresentationState]
    PRESENTATION_STATE_UNAVAILABLE: _ClassVar[PresentationState]
DIRECTION_UNSPECIFIED: Direction
DIRECTION_RECEIVE: Direction
DIRECTION_TRANSMIT: Direction
QUALITY_UNSPECIFIED: Quality
QUALITY_VALID: Quality
QUALITY_INVALID_VALUE: Quality
QUALITY_UNKNOWN: Quality
COMPLETION_UNSPECIFIED: Completion
COMPLETION_COMPLETE: Completion
COMPLETION_INCOMPLETE: Completion
JOB_STATE_UNSPECIFIED: JobState
JOB_STATE_QUEUED: JobState
JOB_STATE_RUNNING: JobState
JOB_STATE_COMPLETED: JobState
JOB_STATE_NO_CHANGE: JobState
JOB_STATE_REJECTED: JobState
JOB_STATE_FAILED: JobState
WEBHOOK_EVENT_TYPE_UNSPECIFIED: WebhookEventType
WEBHOOK_EVENT_TYPE_TRAINING_FAILURE: WebhookEventType
WEBHOOK_EVENT_TYPE_CANDIDATE_REJECTION: WebhookEventType
WEBHOOK_EVENT_TYPE_DEMOTION: WebhookEventType
WEBHOOK_EVENT_TYPE_ROLLBACK: WebhookEventType
GLOBAL_AUTHORITY_UNSPECIFIED: GlobalAuthority
GLOBAL_AUTHORITY_FALLBACK: GlobalAuthority
GLOBAL_AUTHORITY_ACTIVE: GlobalAuthority
GLOBAL_AUTHORITY_HARD_FAULT: GlobalAuthority
FEATURE_AUTHORITY_UNSPECIFIED: FeatureAuthority
FEATURE_AUTHORITY_FALLBACK: FeatureAuthority
FEATURE_AUTHORITY_ARMING: FeatureAuthority
FEATURE_AUTHORITY_ACTIVE: FeatureAuthority
COMMAND_SOURCE_UNSPECIFIED: CommandSource
COMMAND_SOURCE_CONTROLLER_LOCAL_FALLBACK: CommandSource
COMMAND_SOURCE_DETERMINISTIC: CommandSource
COMMAND_SOURCE_MODEL_OPTIMIZED: CommandSource
COMMAND_SOURCE_EXPERIMENT: CommandSource
DIAGNOSTIC_HEALTH_STATE_UNSPECIFIED: DiagnosticHealthState
DIAGNOSTIC_HEALTH_STATE_UNKNOWN: DiagnosticHealthState
DIAGNOSTIC_HEALTH_STATE_HEALTHY: DiagnosticHealthState
DIAGNOSTIC_HEALTH_STATE_UNHEALTHY: DiagnosticHealthState
DIAGNOSTIC_UNKNOWN_REASON_UNSPECIFIED: DiagnosticUnknownReason
DIAGNOSTIC_UNKNOWN_REASON_NEVER_OBSERVED: DiagnosticUnknownReason
DIAGNOSTIC_UNKNOWN_REASON_NOT_EXPECTED_IN_FALLBACK: DiagnosticUnknownReason
DIAGNOSTIC_UNKNOWN_REASON_NOT_RENEWING: DiagnosticUnknownReason
DIAGNOSTIC_UNKNOWN_REASON_NO_OUTSTANDING_COMMAND: DiagnosticUnknownReason
DIAGNOSTIC_UNKNOWN_REASON_AWAITING_EVIDENCE: DiagnosticUnknownReason
RUN_STORAGE_HEALTH_UNSPECIFIED: RunStorageHealth
RUN_STORAGE_HEALTH_UNKNOWN: RunStorageHealth
RUN_STORAGE_HEALTH_HEALTHY: RunStorageHealth
RUN_STORAGE_HEALTH_DEGRADED: RunStorageHealth
PRESENTATION_STATE_UNSPECIFIED: PresentationState
PRESENTATION_STATE_CURRENT: PresentationState
PRESENTATION_STATE_STALE: PresentationState
PRESENTATION_STATE_UNAVAILABLE: PresentationState

class RunEvent(_message.Message):
    __slots__ = ("schema_major", "schema_minor", "run_id", "sequence", "monotonic_ns", "source_monotonic_ns", "ingestion_monotonic_ns", "source", "raw_can", "signal_observation", "condition_transition", "authority_transition", "control_decision", "experiment_event", "lifecycle_event", "storage_event", "time_anchor", "annotation")
    SCHEMA_MAJOR_FIELD_NUMBER: _ClassVar[int]
    SCHEMA_MINOR_FIELD_NUMBER: _ClassVar[int]
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    SEQUENCE_FIELD_NUMBER: _ClassVar[int]
    MONOTONIC_NS_FIELD_NUMBER: _ClassVar[int]
    SOURCE_MONOTONIC_NS_FIELD_NUMBER: _ClassVar[int]
    INGESTION_MONOTONIC_NS_FIELD_NUMBER: _ClassVar[int]
    SOURCE_FIELD_NUMBER: _ClassVar[int]
    RAW_CAN_FIELD_NUMBER: _ClassVar[int]
    SIGNAL_OBSERVATION_FIELD_NUMBER: _ClassVar[int]
    CONDITION_TRANSITION_FIELD_NUMBER: _ClassVar[int]
    AUTHORITY_TRANSITION_FIELD_NUMBER: _ClassVar[int]
    CONTROL_DECISION_FIELD_NUMBER: _ClassVar[int]
    EXPERIMENT_EVENT_FIELD_NUMBER: _ClassVar[int]
    LIFECYCLE_EVENT_FIELD_NUMBER: _ClassVar[int]
    STORAGE_EVENT_FIELD_NUMBER: _ClassVar[int]
    TIME_ANCHOR_FIELD_NUMBER: _ClassVar[int]
    ANNOTATION_FIELD_NUMBER: _ClassVar[int]
    schema_major: int
    schema_minor: int
    run_id: bytes
    sequence: int
    monotonic_ns: int
    source_monotonic_ns: int
    ingestion_monotonic_ns: int
    source: str
    raw_can: RawCanFrame
    signal_observation: SignalObservation
    condition_transition: ConditionTransition
    authority_transition: AuthorityTransition
    control_decision: ControlDecision
    experiment_event: ExperimentEvent
    lifecycle_event: LifecycleEvent
    storage_event: StorageEvent
    time_anchor: TimeAnchor
    annotation: Annotation
    def __init__(self, schema_major: _Optional[int] = ..., schema_minor: _Optional[int] = ..., run_id: _Optional[bytes] = ..., sequence: _Optional[int] = ..., monotonic_ns: _Optional[int] = ..., source_monotonic_ns: _Optional[int] = ..., ingestion_monotonic_ns: _Optional[int] = ..., source: _Optional[str] = ..., raw_can: _Optional[_Union[RawCanFrame, _Mapping]] = ..., signal_observation: _Optional[_Union[SignalObservation, _Mapping]] = ..., condition_transition: _Optional[_Union[ConditionTransition, _Mapping]] = ..., authority_transition: _Optional[_Union[AuthorityTransition, _Mapping]] = ..., control_decision: _Optional[_Union[ControlDecision, _Mapping]] = ..., experiment_event: _Optional[_Union[ExperimentEvent, _Mapping]] = ..., lifecycle_event: _Optional[_Union[LifecycleEvent, _Mapping]] = ..., storage_event: _Optional[_Union[StorageEvent, _Mapping]] = ..., time_anchor: _Optional[_Union[TimeAnchor, _Mapping]] = ..., annotation: _Optional[_Union[Annotation, _Mapping]] = ...) -> None: ...

class RawCanFrame(_message.Message):
    __slots__ = ("channel", "direction", "id", "extended", "fd", "bit_rate_switch", "error_state_indicator", "data", "hardware_timestamp_ns", "receive_overflow_count", "dropped_count")
    CHANNEL_FIELD_NUMBER: _ClassVar[int]
    DIRECTION_FIELD_NUMBER: _ClassVar[int]
    ID_FIELD_NUMBER: _ClassVar[int]
    EXTENDED_FIELD_NUMBER: _ClassVar[int]
    FD_FIELD_NUMBER: _ClassVar[int]
    BIT_RATE_SWITCH_FIELD_NUMBER: _ClassVar[int]
    ERROR_STATE_INDICATOR_FIELD_NUMBER: _ClassVar[int]
    DATA_FIELD_NUMBER: _ClassVar[int]
    HARDWARE_TIMESTAMP_NS_FIELD_NUMBER: _ClassVar[int]
    RECEIVE_OVERFLOW_COUNT_FIELD_NUMBER: _ClassVar[int]
    DROPPED_COUNT_FIELD_NUMBER: _ClassVar[int]
    channel: str
    direction: Direction
    id: int
    extended: bool
    fd: bool
    bit_rate_switch: bool
    error_state_indicator: bool
    data: bytes
    hardware_timestamp_ns: int
    receive_overflow_count: int
    dropped_count: int
    def __init__(self, channel: _Optional[str] = ..., direction: _Optional[_Union[Direction, str]] = ..., id: _Optional[int] = ..., extended: bool = ..., fd: bool = ..., bit_rate_switch: bool = ..., error_state_indicator: bool = ..., data: _Optional[bytes] = ..., hardware_timestamp_ns: _Optional[int] = ..., receive_overflow_count: _Optional[int] = ..., dropped_count: _Optional[int] = ...) -> None: ...

class SignalObservation(_message.Message):
    __slots__ = ("signal", "value", "unit", "reference", "quality", "reason", "source_event_sequence", "decoder_generation", "age_ns")
    SIGNAL_FIELD_NUMBER: _ClassVar[int]
    VALUE_FIELD_NUMBER: _ClassVar[int]
    UNIT_FIELD_NUMBER: _ClassVar[int]
    REFERENCE_FIELD_NUMBER: _ClassVar[int]
    QUALITY_FIELD_NUMBER: _ClassVar[int]
    REASON_FIELD_NUMBER: _ClassVar[int]
    SOURCE_EVENT_SEQUENCE_FIELD_NUMBER: _ClassVar[int]
    DECODER_GENERATION_FIELD_NUMBER: _ClassVar[int]
    AGE_NS_FIELD_NUMBER: _ClassVar[int]
    signal: str
    value: float
    unit: str
    reference: str
    quality: Quality
    reason: str
    source_event_sequence: int
    decoder_generation: int
    age_ns: int
    def __init__(self, signal: _Optional[str] = ..., value: _Optional[float] = ..., unit: _Optional[str] = ..., reference: _Optional[str] = ..., quality: _Optional[_Union[Quality, str]] = ..., reason: _Optional[str] = ..., source_event_sequence: _Optional[int] = ..., decoder_generation: _Optional[int] = ..., age_ns: _Optional[int] = ...) -> None: ...

class ConditionTransition(_message.Message):
    __slots__ = ("encoded",)
    ENCODED_FIELD_NUMBER: _ClassVar[int]
    encoded: str
    def __init__(self, encoded: _Optional[str] = ...) -> None: ...

class AuthorityTransition(_message.Message):
    __slots__ = ("encoded",)
    ENCODED_FIELD_NUMBER: _ClassVar[int]
    encoded: str
    def __init__(self, encoded: _Optional[str] = ...) -> None: ...

class ControlDecision(_message.Message):
    __slots__ = ("encoded",)
    ENCODED_FIELD_NUMBER: _ClassVar[int]
    encoded: str
    def __init__(self, encoded: _Optional[str] = ...) -> None: ...

class ExperimentEvent(_message.Message):
    __slots__ = ("encoded",)
    ENCODED_FIELD_NUMBER: _ClassVar[int]
    encoded: str
    def __init__(self, encoded: _Optional[str] = ...) -> None: ...

class LifecycleEvent(_message.Message):
    __slots__ = ("encoded",)
    ENCODED_FIELD_NUMBER: _ClassVar[int]
    encoded: str
    def __init__(self, encoded: _Optional[str] = ...) -> None: ...

class StorageEvent(_message.Message):
    __slots__ = ("encoded",)
    ENCODED_FIELD_NUMBER: _ClassVar[int]
    encoded: str
    def __init__(self, encoded: _Optional[str] = ...) -> None: ...

class TimeAnchor(_message.Message):
    __slots__ = ("encoded",)
    ENCODED_FIELD_NUMBER: _ClassVar[int]
    encoded: str
    def __init__(self, encoded: _Optional[str] = ...) -> None: ...

class Annotation(_message.Message):
    __slots__ = ("encoded",)
    ENCODED_FIELD_NUMBER: _ClassVar[int]
    encoded: str
    def __init__(self, encoded: _Optional[str] = ...) -> None: ...

class RunManifest(_message.Message):
    __slots__ = ("schema_version", "run_id", "completion", "incomplete_reason", "first_sequence", "last_sequence", "chunk_file", "chunk_sha256", "chunks", "dropped_record_count", "configuration_generation", "configuration_sha256", "model_bundle_digest", "protocol_major", "firmware_generation", "decoder_generation", "model_abi", "model_input_signals", "model_history_length", "sample_period_ms", "command_lattice", "maximum_calibration_error")
    SCHEMA_VERSION_FIELD_NUMBER: _ClassVar[int]
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    COMPLETION_FIELD_NUMBER: _ClassVar[int]
    INCOMPLETE_REASON_FIELD_NUMBER: _ClassVar[int]
    FIRST_SEQUENCE_FIELD_NUMBER: _ClassVar[int]
    LAST_SEQUENCE_FIELD_NUMBER: _ClassVar[int]
    CHUNK_FILE_FIELD_NUMBER: _ClassVar[int]
    CHUNK_SHA256_FIELD_NUMBER: _ClassVar[int]
    CHUNKS_FIELD_NUMBER: _ClassVar[int]
    DROPPED_RECORD_COUNT_FIELD_NUMBER: _ClassVar[int]
    CONFIGURATION_GENERATION_FIELD_NUMBER: _ClassVar[int]
    CONFIGURATION_SHA256_FIELD_NUMBER: _ClassVar[int]
    MODEL_BUNDLE_DIGEST_FIELD_NUMBER: _ClassVar[int]
    PROTOCOL_MAJOR_FIELD_NUMBER: _ClassVar[int]
    FIRMWARE_GENERATION_FIELD_NUMBER: _ClassVar[int]
    DECODER_GENERATION_FIELD_NUMBER: _ClassVar[int]
    MODEL_ABI_FIELD_NUMBER: _ClassVar[int]
    MODEL_INPUT_SIGNALS_FIELD_NUMBER: _ClassVar[int]
    MODEL_HISTORY_LENGTH_FIELD_NUMBER: _ClassVar[int]
    SAMPLE_PERIOD_MS_FIELD_NUMBER: _ClassVar[int]
    COMMAND_LATTICE_FIELD_NUMBER: _ClassVar[int]
    MAXIMUM_CALIBRATION_ERROR_FIELD_NUMBER: _ClassVar[int]
    schema_version: int
    run_id: str
    completion: Completion
    incomplete_reason: str
    first_sequence: int
    last_sequence: int
    chunk_file: str
    chunk_sha256: str
    chunks: _containers.RepeatedCompositeFieldContainer[RunChunk]
    dropped_record_count: int
    configuration_generation: int
    configuration_sha256: str
    model_bundle_digest: str
    protocol_major: int
    firmware_generation: int
    decoder_generation: int
    model_abi: str
    model_input_signals: _containers.RepeatedScalarFieldContainer[str]
    model_history_length: int
    sample_period_ms: int
    command_lattice: _containers.RepeatedScalarFieldContainer[int]
    maximum_calibration_error: float
    def __init__(self, schema_version: _Optional[int] = ..., run_id: _Optional[str] = ..., completion: _Optional[_Union[Completion, str]] = ..., incomplete_reason: _Optional[str] = ..., first_sequence: _Optional[int] = ..., last_sequence: _Optional[int] = ..., chunk_file: _Optional[str] = ..., chunk_sha256: _Optional[str] = ..., chunks: _Optional[_Iterable[_Union[RunChunk, _Mapping]]] = ..., dropped_record_count: _Optional[int] = ..., configuration_generation: _Optional[int] = ..., configuration_sha256: _Optional[str] = ..., model_bundle_digest: _Optional[str] = ..., protocol_major: _Optional[int] = ..., firmware_generation: _Optional[int] = ..., decoder_generation: _Optional[int] = ..., model_abi: _Optional[str] = ..., model_input_signals: _Optional[_Iterable[str]] = ..., model_history_length: _Optional[int] = ..., sample_period_ms: _Optional[int] = ..., command_lattice: _Optional[_Iterable[int]] = ..., maximum_calibration_error: _Optional[float] = ...) -> None: ...

class RunChunk(_message.Message):
    __slots__ = ("file", "sha256")
    FILE_FIELD_NUMBER: _ClassVar[int]
    SHA256_FIELD_NUMBER: _ClassVar[int]
    file: str
    sha256: str
    def __init__(self, file: _Optional[str] = ..., sha256: _Optional[str] = ...) -> None: ...

class Health(_message.Message):
    __slots__ = ("status",)
    STATUS_FIELD_NUMBER: _ClassVar[int]
    status: str
    def __init__(self, status: _Optional[str] = ...) -> None: ...

class ReconcileRequest(_message.Message):
    __slots__ = ("schema_version", "completed_run_digests", "active_model_digest", "staged_model_digest", "rejected_model_digests")
    SCHEMA_VERSION_FIELD_NUMBER: _ClassVar[int]
    COMPLETED_RUN_DIGESTS_FIELD_NUMBER: _ClassVar[int]
    ACTIVE_MODEL_DIGEST_FIELD_NUMBER: _ClassVar[int]
    STAGED_MODEL_DIGEST_FIELD_NUMBER: _ClassVar[int]
    REJECTED_MODEL_DIGESTS_FIELD_NUMBER: _ClassVar[int]
    schema_version: int
    completed_run_digests: _containers.RepeatedScalarFieldContainer[str]
    active_model_digest: str
    staged_model_digest: str
    rejected_model_digests: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, schema_version: _Optional[int] = ..., completed_run_digests: _Optional[_Iterable[str]] = ..., active_model_digest: _Optional[str] = ..., staged_model_digest: _Optional[str] = ..., rejected_model_digests: _Optional[_Iterable[str]] = ...) -> None: ...

class ReconcileResponse(_message.Message):
    __slots__ = ("missing_run_digests", "desired_model_digest")
    MISSING_RUN_DIGESTS_FIELD_NUMBER: _ClassVar[int]
    DESIRED_MODEL_DIGEST_FIELD_NUMBER: _ClassVar[int]
    missing_run_digests: _containers.RepeatedScalarFieldContainer[str]
    desired_model_digest: str
    def __init__(self, missing_run_digests: _Optional[_Iterable[str]] = ..., desired_model_digest: _Optional[str] = ...) -> None: ...

class CompletedRun(_message.Message):
    __slots__ = ("run_digest",)
    RUN_DIGEST_FIELD_NUMBER: _ClassVar[int]
    run_digest: str
    def __init__(self, run_digest: _Optional[str] = ...) -> None: ...

class JobRequest(_message.Message):
    __slots__ = ("run_digests", "recipe")
    RUN_DIGESTS_FIELD_NUMBER: _ClassVar[int]
    RECIPE_FIELD_NUMBER: _ClassVar[int]
    run_digests: _containers.RepeatedScalarFieldContainer[str]
    recipe: str
    def __init__(self, run_digests: _Optional[_Iterable[str]] = ..., recipe: _Optional[str] = ...) -> None: ...

class JobStarted(_message.Message):
    __slots__ = ("job_id", "state")
    JOB_ID_FIELD_NUMBER: _ClassVar[int]
    STATE_FIELD_NUMBER: _ClassVar[int]
    job_id: str
    state: JobState
    def __init__(self, job_id: _Optional[str] = ..., state: _Optional[_Union[JobState, str]] = ...) -> None: ...

class JobStatus(_message.Message):
    __slots__ = ("id", "state", "recipe", "run_digests", "pid", "stdout_path", "stderr_path", "artifact_digest", "terminal_summary", "created_at")
    ID_FIELD_NUMBER: _ClassVar[int]
    STATE_FIELD_NUMBER: _ClassVar[int]
    RECIPE_FIELD_NUMBER: _ClassVar[int]
    RUN_DIGESTS_FIELD_NUMBER: _ClassVar[int]
    PID_FIELD_NUMBER: _ClassVar[int]
    STDOUT_PATH_FIELD_NUMBER: _ClassVar[int]
    STDERR_PATH_FIELD_NUMBER: _ClassVar[int]
    ARTIFACT_DIGEST_FIELD_NUMBER: _ClassVar[int]
    TERMINAL_SUMMARY_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    id: str
    state: JobState
    recipe: str
    run_digests: _containers.RepeatedScalarFieldContainer[str]
    pid: int
    stdout_path: str
    stderr_path: str
    artifact_digest: str
    terminal_summary: str
    created_at: str
    def __init__(self, id: _Optional[str] = ..., state: _Optional[_Union[JobState, str]] = ..., recipe: _Optional[str] = ..., run_digests: _Optional[_Iterable[str]] = ..., pid: _Optional[int] = ..., stdout_path: _Optional[str] = ..., stderr_path: _Optional[str] = ..., artifact_digest: _Optional[str] = ..., terminal_summary: _Optional[str] = ..., created_at: _Optional[str] = ...) -> None: ...

class ModelBundleManifest(_message.Message):
    __slots__ = ("schema_version", "onnx_sha256", "signal_order", "units", "sample_period_ms", "history_length", "horizons", "output_order", "command_lattice", "compatibility", "input_ranges", "normalization", "calibration_error")
    SCHEMA_VERSION_FIELD_NUMBER: _ClassVar[int]
    ONNX_SHA256_FIELD_NUMBER: _ClassVar[int]
    SIGNAL_ORDER_FIELD_NUMBER: _ClassVar[int]
    UNITS_FIELD_NUMBER: _ClassVar[int]
    SAMPLE_PERIOD_MS_FIELD_NUMBER: _ClassVar[int]
    HISTORY_LENGTH_FIELD_NUMBER: _ClassVar[int]
    HORIZONS_FIELD_NUMBER: _ClassVar[int]
    OUTPUT_ORDER_FIELD_NUMBER: _ClassVar[int]
    COMMAND_LATTICE_FIELD_NUMBER: _ClassVar[int]
    COMPATIBILITY_FIELD_NUMBER: _ClassVar[int]
    INPUT_RANGES_FIELD_NUMBER: _ClassVar[int]
    NORMALIZATION_FIELD_NUMBER: _ClassVar[int]
    CALIBRATION_ERROR_FIELD_NUMBER: _ClassVar[int]
    schema_version: int
    onnx_sha256: str
    signal_order: _containers.RepeatedScalarFieldContainer[str]
    units: _containers.RepeatedScalarFieldContainer[str]
    sample_period_ms: int
    history_length: int
    horizons: _containers.RepeatedScalarFieldContainer[int]
    output_order: _containers.RepeatedScalarFieldContainer[str]
    command_lattice: _containers.RepeatedScalarFieldContainer[int]
    compatibility: ModelCompatibility
    input_ranges: _containers.RepeatedCompositeFieldContainer[ModelInputRange]
    normalization: _containers.RepeatedCompositeFieldContainer[ModelNormalization]
    calibration_error: float
    def __init__(self, schema_version: _Optional[int] = ..., onnx_sha256: _Optional[str] = ..., signal_order: _Optional[_Iterable[str]] = ..., units: _Optional[_Iterable[str]] = ..., sample_period_ms: _Optional[int] = ..., history_length: _Optional[int] = ..., horizons: _Optional[_Iterable[int]] = ..., output_order: _Optional[_Iterable[str]] = ..., command_lattice: _Optional[_Iterable[int]] = ..., compatibility: _Optional[_Union[ModelCompatibility, _Mapping]] = ..., input_ranges: _Optional[_Iterable[_Union[ModelInputRange, _Mapping]]] = ..., normalization: _Optional[_Iterable[_Union[ModelNormalization, _Mapping]]] = ..., calibration_error: _Optional[float] = ...) -> None: ...

class ModelCompatibility(_message.Message):
    __slots__ = ("model_abi", "input_shape", "derivation_digest", "input_runs", "maximum_parity_error", "held_out_mse")
    MODEL_ABI_FIELD_NUMBER: _ClassVar[int]
    INPUT_SHAPE_FIELD_NUMBER: _ClassVar[int]
    DERIVATION_DIGEST_FIELD_NUMBER: _ClassVar[int]
    INPUT_RUNS_FIELD_NUMBER: _ClassVar[int]
    MAXIMUM_PARITY_ERROR_FIELD_NUMBER: _ClassVar[int]
    HELD_OUT_MSE_FIELD_NUMBER: _ClassVar[int]
    model_abi: str
    input_shape: _containers.RepeatedScalarFieldContainer[int]
    derivation_digest: str
    input_runs: _containers.RepeatedScalarFieldContainer[str]
    maximum_parity_error: float
    held_out_mse: float
    def __init__(self, model_abi: _Optional[str] = ..., input_shape: _Optional[_Iterable[int]] = ..., derivation_digest: _Optional[str] = ..., input_runs: _Optional[_Iterable[str]] = ..., maximum_parity_error: _Optional[float] = ..., held_out_mse: _Optional[float] = ...) -> None: ...

class ModelInputRange(_message.Message):
    __slots__ = ("minimum", "maximum")
    MINIMUM_FIELD_NUMBER: _ClassVar[int]
    MAXIMUM_FIELD_NUMBER: _ClassVar[int]
    minimum: float
    maximum: float
    def __init__(self, minimum: _Optional[float] = ..., maximum: _Optional[float] = ...) -> None: ...

class ModelNormalization(_message.Message):
    __slots__ = ("mean", "scale")
    MEAN_FIELD_NUMBER: _ClassVar[int]
    SCALE_FIELD_NUMBER: _ClassVar[int]
    mean: float
    scale: float
    def __init__(self, mean: _Optional[float] = ..., scale: _Optional[float] = ...) -> None: ...

class WebhookEvent(_message.Message):
    __slots__ = ("schema_version", "event_id", "event_type", "occurred_at", "job_id", "artifact_digest", "summary")
    SCHEMA_VERSION_FIELD_NUMBER: _ClassVar[int]
    EVENT_ID_FIELD_NUMBER: _ClassVar[int]
    EVENT_TYPE_FIELD_NUMBER: _ClassVar[int]
    OCCURRED_AT_FIELD_NUMBER: _ClassVar[int]
    JOB_ID_FIELD_NUMBER: _ClassVar[int]
    ARTIFACT_DIGEST_FIELD_NUMBER: _ClassVar[int]
    SUMMARY_FIELD_NUMBER: _ClassVar[int]
    schema_version: int
    event_id: str
    event_type: WebhookEventType
    occurred_at: str
    job_id: str
    artifact_digest: str
    summary: str
    def __init__(self, schema_version: _Optional[int] = ..., event_id: _Optional[str] = ..., event_type: _Optional[_Union[WebhookEventType, str]] = ..., occurred_at: _Optional[str] = ..., job_id: _Optional[str] = ..., artifact_digest: _Optional[str] = ..., summary: _Optional[str] = ...) -> None: ...

class DiagnosticsSnapshot(_message.Message):
    __slots__ = ("schema_version", "runtime_update_age_ms", "runtime_update_stale_after_ms", "global_authority", "feature_authority", "command_source", "accepted_radiator_split_command", "coolant_temperature", "intake_air_temperature", "controller_runtime_lease_health", "controller_command_ack_health", "run_storage_health")
    SCHEMA_VERSION_FIELD_NUMBER: _ClassVar[int]
    RUNTIME_UPDATE_AGE_MS_FIELD_NUMBER: _ClassVar[int]
    RUNTIME_UPDATE_STALE_AFTER_MS_FIELD_NUMBER: _ClassVar[int]
    GLOBAL_AUTHORITY_FIELD_NUMBER: _ClassVar[int]
    FEATURE_AUTHORITY_FIELD_NUMBER: _ClassVar[int]
    COMMAND_SOURCE_FIELD_NUMBER: _ClassVar[int]
    ACCEPTED_RADIATOR_SPLIT_COMMAND_FIELD_NUMBER: _ClassVar[int]
    COOLANT_TEMPERATURE_FIELD_NUMBER: _ClassVar[int]
    INTAKE_AIR_TEMPERATURE_FIELD_NUMBER: _ClassVar[int]
    CONTROLLER_RUNTIME_LEASE_HEALTH_FIELD_NUMBER: _ClassVar[int]
    CONTROLLER_COMMAND_ACK_HEALTH_FIELD_NUMBER: _ClassVar[int]
    RUN_STORAGE_HEALTH_FIELD_NUMBER: _ClassVar[int]
    schema_version: int
    runtime_update_age_ms: int
    runtime_update_stale_after_ms: int
    global_authority: GlobalAuthority
    feature_authority: FeatureAuthority
    command_source: CommandSource
    accepted_radiator_split_command: AcceptedRadiatorSplitCommand
    coolant_temperature: ObservedTemperature
    intake_air_temperature: ObservedTemperature
    controller_runtime_lease_health: DiagnosticStatus
    controller_command_ack_health: DiagnosticStatus
    run_storage_health: RunStorageHealth
    def __init__(self, schema_version: _Optional[int] = ..., runtime_update_age_ms: _Optional[int] = ..., runtime_update_stale_after_ms: _Optional[int] = ..., global_authority: _Optional[_Union[GlobalAuthority, str]] = ..., feature_authority: _Optional[_Union[FeatureAuthority, str]] = ..., command_source: _Optional[_Union[CommandSource, str]] = ..., accepted_radiator_split_command: _Optional[_Union[AcceptedRadiatorSplitCommand, _Mapping]] = ..., coolant_temperature: _Optional[_Union[ObservedTemperature, _Mapping]] = ..., intake_air_temperature: _Optional[_Union[ObservedTemperature, _Mapping]] = ..., controller_runtime_lease_health: _Optional[_Union[DiagnosticStatus, _Mapping]] = ..., controller_command_ack_health: _Optional[_Union[DiagnosticStatus, _Mapping]] = ..., run_storage_health: _Optional[_Union[RunStorageHealth, str]] = ...) -> None: ...

class DiagnosticStatus(_message.Message):
    __slots__ = ("state", "reason")
    STATE_FIELD_NUMBER: _ClassVar[int]
    REASON_FIELD_NUMBER: _ClassVar[int]
    state: DiagnosticHealthState
    reason: DiagnosticUnknownReason
    def __init__(self, state: _Optional[_Union[DiagnosticHealthState, str]] = ..., reason: _Optional[_Union[DiagnosticUnknownReason, str]] = ...) -> None: ...

class ObservedTemperature(_message.Message):
    __slots__ = ("degrees_celsius", "observation_age_ms")
    DEGREES_CELSIUS_FIELD_NUMBER: _ClassVar[int]
    OBSERVATION_AGE_MS_FIELD_NUMBER: _ClassVar[int]
    degrees_celsius: float
    observation_age_ms: int
    def __init__(self, degrees_celsius: _Optional[float] = ..., observation_age_ms: _Optional[int] = ...) -> None: ...

class AcceptedRadiatorSplitCommand(_message.Message):
    __slots__ = ("basis_points", "source")
    BASIS_POINTS_FIELD_NUMBER: _ClassVar[int]
    SOURCE_FIELD_NUMBER: _ClassVar[int]
    basis_points: int
    source: CommandSource
    def __init__(self, basis_points: _Optional[int] = ..., source: _Optional[_Union[CommandSource, str]] = ...) -> None: ...

class StatusEnvelope(_message.Message):
    __slots__ = ("schema_version", "state", "consecutive_failures", "last_success_age_ms", "snapshot")
    SCHEMA_VERSION_FIELD_NUMBER: _ClassVar[int]
    STATE_FIELD_NUMBER: _ClassVar[int]
    CONSECUTIVE_FAILURES_FIELD_NUMBER: _ClassVar[int]
    LAST_SUCCESS_AGE_MS_FIELD_NUMBER: _ClassVar[int]
    SNAPSHOT_FIELD_NUMBER: _ClassVar[int]
    schema_version: int
    state: PresentationState
    consecutive_failures: int
    last_success_age_ms: int
    snapshot: DiagnosticsSnapshot
    def __init__(self, schema_version: _Optional[int] = ..., state: _Optional[_Union[PresentationState, str]] = ..., consecutive_failures: _Optional[int] = ..., last_success_age_ms: _Optional[int] = ..., snapshot: _Optional[_Union[DiagnosticsSnapshot, _Mapping]] = ...) -> None: ...
