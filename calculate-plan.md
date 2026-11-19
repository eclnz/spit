# File-backed calculations for issue #265

[Issue #265](https://github.com/eclnz/spit/issues/265) proposes arithmetic over numeric inputs. [Issue #253](https://github.com/eclnz/spit/issues/253) defines numeric values as files read by `@scalar`. This draft uses that contract.

```spit
source reference : Scalar .txt [sub]
source gain : Scalar .txt [sub]
operation calibrate(reference: Scalar, gain: Scalar) -> Scalar .txt
calculate calibrate: round(reference * exp(gain), 1)
m0 = calibrate(reference, gain)
operation consume(m0: Scalar) -> Text .txt
command consume: printf '%s\n' @scalar(m0, min=0) > {@output}
result = consume(m0)
```

For input files containing `80` and `1.844439727`, the calibration job writes `506.0` and a newline. Normal dependency and freshness rules apply. Plans show the expression and input paths. Bounds apply when a consumer reads the result.

- [x] Parse and validate the formula in the compiler, and bind its named ports and sole file output. `e0cfe2e`
- [x] Encode the validated expression as JSON prefix arrays in DAG version 18. Operator names occur only at the head of an array. In operand position, a string names a port. `e0cfe2e`
- [x] Evaluate in the runner using the scalar reader and ordinary job lifecycle. Use binary64, right-associative exponentiation, and ties-to-even rounding with 0–15 places. Keep ordinary input and output checks and verify commands. `e0cfe2e`
- [x] Verify the compiler, local runner, Slurm worker and companion grammar. The guide example writes and consumes `506.0`. Both compiler benchmarks pass against `2a44dbe`. SPIT `e0cfe2e`, spit-vscode `6b284f0`.
- [x] Give unary and binary operators fixed operand counts in the compiler and verify that the DAG loader rejects too few or too many operands. `d060df0`
- [x] Check that a changed scalar reruns its calculation and that identical rounded bytes keep the downstream job current. `d060df0`

Call-site quality-control annotations ([#288](https://github.com/eclnz/spit/issues/288)) can apply to the existing input ports of a calculation job. The calculation payload adds no separate input mechanism. That issue follows phase 2 of [#203](https://github.com/eclnz/spit/issues/203).
