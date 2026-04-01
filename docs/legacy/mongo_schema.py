#%%
"""Preserved Mongo schema helper from ayxm.

This module is kept as reference material for the Mongo schema export pattern
used in the previous Python repository.
"""

import pandas as pd


def load_schema(path: str) -> pd.DataFrame:
    return pd.read_csv(path)


def schema_records(path: str):
    df = load_schema(path)
    return df.to_dict(orient="records")

