use bloxide_calibration::{
    Access, ByteOrder, DescriptorError, DescriptorSpec, EncodedValue, MapError, MappedVariable,
    NumericLimits, Registry, ScalarValue, Scaling, VariableDescriptor, WireType, WritePolicy,
};

fn descriptor(
    id: u32,
    symbol: &'static str,
    wire_type: WireType,
    limits: Option<NumericLimits>,
    default: Option<ScalarValue>,
    access: Access,
) -> VariableDescriptor {
    VariableDescriptor::try_new(DescriptorSpec {
        variable_id: id,
        symbol,
        owner: "fixture",
        description: "fixture variable",
        wire_type,
        byte_order: ByteOrder::Little,
        unit: "ms",
        scaling: Scaling::IDENTITY,
        limits,
        default,
        access,
        write_policy: if access.is_writable() {
            WritePolicy::CompleteScalar
        } else {
            WritePolicy::ReadOnly
        },
        schema_version: 1,
    })
    .unwrap()
}

#[test]
fn registry_rejects_bad_defaults_duplicates_overlap_and_overflow() {
    let limits = NumericLimits::try_new(ScalarValue::U16(100), ScalarValue::U16(10_000)).unwrap();
    let bad_default = VariableDescriptor::try_new(DescriptorSpec {
        variable_id: 1,
        symbol: "led.period_ms",
        owner: "led",
        description: "period",
        wire_type: WireType::U16,
        byte_order: ByteOrder::Little,
        unit: "ms",
        scaling: Scaling::IDENTITY,
        limits: Some(limits),
        default: Some(ScalarValue::U16(99)),
        access: Access::CalibrationReadWrite,
        write_policy: WritePolicy::CompleteScalar,
        schema_version: 1,
    });
    assert_eq!(
        bad_default.unwrap_err(),
        DescriptorError::DefaultOutOfBounds
    );

    let period = descriptor(
        1,
        "led.period_ms",
        WireType::U16,
        Some(limits),
        Some(ScalarValue::U16(1000)),
        Access::CalibrationReadWrite,
    );
    let duplicate = MappedVariable::new(0, 0x1002, period);
    assert_eq!(
        Registry::try_new([MappedVariable::new(0, 0x1000, period), duplicate]).err(),
        Some(MapError::DuplicateVariableId)
    );

    let duty = descriptor(
        2,
        "led.duty_permille",
        WireType::U16,
        Some(NumericLimits::try_new(ScalarValue::U16(0), ScalarValue::U16(1000)).unwrap()),
        Some(ScalarValue::U16(500)),
        Access::CalibrationReadWrite,
    );
    assert_eq!(
        Registry::try_new([
            MappedVariable::new(0, 0x1000, period),
            MappedVariable::new(0, 0x1001, duty),
        ])
        .err(),
        Some(MapError::Overlap)
    );
    assert_eq!(
        Registry::try_new([
            MappedVariable::new(0, u32::MAX, period),
            MappedVariable::new(0, 0x1002, duty),
        ])
        .err(),
        Some(MapError::AddressOverflow)
    );
}

#[test]
fn map_rejects_partial_cross_region_and_read_only_writes() {
    let period = descriptor(
        1,
        "led.period_ms",
        WireType::U16,
        Some(NumericLimits::try_new(ScalarValue::U16(100), ScalarValue::U16(10_000)).unwrap()),
        Some(ScalarValue::U16(1000)),
        Access::CalibrationReadWrite,
    );
    let uptime = descriptor(
        2,
        "led.uptime_ms",
        WireType::U32,
        None,
        None,
        Access::MeasurementReadOnly,
    );
    let registry = Registry::try_new([
        MappedVariable::new(0, 0x1000, period),
        MappedVariable::new(0, 0x2000, uptime),
    ])
    .unwrap();

    let one = EncodedValue::try_from_slice(&[1]).unwrap();
    assert_eq!(
        registry.resolve_write(0, 0x1000, &one).unwrap_err(),
        MapError::PartialWrite
    );
    let four = EncodedValue::try_from_slice(&[1, 2, 3, 4]).unwrap();
    assert_eq!(
        registry.resolve_write(0, 0x1000, &four).unwrap_err(),
        MapError::CrossesRegion
    );
    assert_eq!(
        registry.resolve_write(0, 0x2000, &four).unwrap_err(),
        MapError::ReadOnly
    );
    assert_eq!(
        registry.resolve_read(0, 0x1001, 2).unwrap_err(),
        MapError::CrossesRegion
    );
    assert_eq!(
        registry.resolve_read(1, 0x1000, 1).unwrap_err(),
        MapError::Unmapped
    );
}

#[cfg(feature = "std")]
#[test]
fn exchange_json_round_trips_type_units_defaults_and_permissions() {
    use bloxide_calibration::exchange::DescriptorDocument;

    let period = descriptor(
        0x1001,
        "led.period_ms",
        WireType::U16,
        Some(NumericLimits::try_new(ScalarValue::U16(100), ScalarValue::U16(10_000)).unwrap()),
        Some(ScalarValue::U16(1000)),
        Access::CalibrationReadWrite,
    );
    let uptime = descriptor(
        0x2001,
        "thermal.uptime_ms",
        WireType::U32,
        None,
        None,
        Access::MeasurementReadOnly,
    );
    let registry = Registry::try_new([
        MappedVariable::new(0, 0x1000, period),
        MappedVariable::new(0, 0x2000, uptime),
    ])
    .unwrap();
    let document = DescriptorDocument::from_registry(&registry);
    let json = document.to_json().unwrap();
    let decoded = DescriptorDocument::from_json(&json).unwrap();
    assert_eq!(decoded, document);
    assert!(json.contains("calibration_read_write"));
    assert!(json.contains("measurement_read_only"));
    assert!(json.contains("\"unit\": \"ms\""));
    assert!(json.contains("\"u16\""));
}

#[test]
fn invalid_float_metadata_is_rejected() {
    assert_eq!(
        NumericLimits::try_new(ScalarValue::F32(f32::NAN), ScalarValue::F32(10.0)),
        Err(DescriptorError::NonFinite)
    );
    let result = VariableDescriptor::try_new(DescriptorSpec {
        variable_id: 3,
        symbol: "thermal.limit",
        owner: "thermal",
        description: "thermal limit",
        wire_type: WireType::F32,
        byte_order: ByteOrder::Little,
        unit: "degC",
        scaling: Scaling::IDENTITY,
        limits: None,
        default: Some(ScalarValue::F32(f32::INFINITY)),
        access: Access::CalibrationReadWrite,
        write_policy: WritePolicy::CompleteScalar,
        schema_version: 1,
    });
    assert_eq!(result.unwrap_err(), DescriptorError::NonFinite);
}
