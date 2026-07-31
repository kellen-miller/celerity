from celerity_home import SCHEMA_VERSION


def test_home_schema_is_v1() -> None:
    assert SCHEMA_VERSION == "1"
