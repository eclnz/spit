# Reusable steps

An operation can be carried out by steps instead of a command: its header ends in `:`, and its steps are indented beneath it (see the [language reference](../manual/operations.md#operations-carried-out-by-steps)). A library declares such an operation once, and a pipeline imports it and calls it like any other. Two examples under [`examples/composites/`](https://github.com/eclnz/spit/tree/dev/examples/composites) show it.

[`mrtrix_dwi.spit`](https://github.com/eclnz/spit/blob/dev/examples/composites/mrtrix/mrtrix_dwi.spit) declares the MRtrix3 operations of the ACT example's preprocessing, and two operations carried out by them: `clean_dwi_session`, from a session's DWI runs and reverse phase-encoded b=0 to its corrected series and mean b=0, and `register_to_dwi`, which moves an anatomical image onto that b=0. [`act.spit`](https://github.com/eclnz/spit/blob/dev/examples/composites/mrtrix/act.spit) calls each once:

```text
use clean_dwi_session, register_to_dwi from mrtrix_dwi.spit as mrx

stage preprocess:
    corrected_dwi, session_b0 = mrx::clean_dwi_session(raw_dwi, dwi_bvec, dwi_bval, dwi_json, reverse_b0, reverse_b0_json)

stage anatomy:
    t1w_dwi, t1_to_dwi = mrx::register_to_dwi(t1w, session_b0)
```

Over the ACT example's mock data, the session call makes the 48 jobs the hand-written `preprocess` stage makes, the same number of each operation. `spit dag examples/composites/mrtrix/act.spitin --counts` lists them under the call:

```text
jobs  step                                                   stage
      corrected_dwi, session_b0 = mrx::clean_dwi_session     preprocess
   7    corrected_dwi::imported = mrx::import_dwi            preprocess
   3    corrected_dwi::reverse_mif = mrx::import_reverse_b0  preprocess
   7    corrected_dwi::denoised = mrx::denoise               preprocess
```

The products the body makes for itself are filed under the call's first output, as `corrected_dwi::imported`, written `corrected_dwi.imported` in a path. `--commands` starts each job with the call and the library line it came from:

```text
Job 49  mrx::export_nifti  [anatomy]
  from:   t1w_dwi = mrx::register_to_dwi (act.spit line 24), mrtrix_dwi.spit line 65
  run:    mrconvert derivatives/preprocess/session_b0/sub=01__ses=01.mif derivatives/anatomy/t1w_dwi.b0_nifti/sub=01__ses=01.nii.gz
```

An error in a step the call makes is reported at the call, with the library line beneath it. Without the reverse b=0 of `sub-02`'s first session, `spit dag` stops at the call:

```text
error: line 21, column 33: in `corrected_dwi, session_b0 = mrx::clean_dwi_session(...)`: no `corrected_dwi::reverse_mif` artifact for input `reverse` of `mrx::combine_pe_pair` at [ses=01,sub=02]
  --> mrtrix_dwi.spit: line 45, column 15: the step in the body of `mrx::clean_dwi_session`
```

[`germline.spit`](https://github.com/eclnz/spit/blob/dev/examples/composites/germline/germline.spit) aligns one sample's lanes with BWA, sorts and merges them with samtools, marks duplicates with GATK, which writes the BAM's index beside it, and calls a GVCF. [`somatic.spit`](https://github.com/eclnz/spit/blob/dev/examples/composites/germline/somatic.spit) calls it for a tumour and its matched normal, 7 jobs each. The two calls have the same steps but file their products apart, as `normal_bam.aligned` and `tumour_bam.aligned`. The check `align_sample` puts on its `bam` output runs beside the one `mark_duplicates` puts on its own, on the job that makes the BAM:

```text
  check:  test -s out/normal_bam/patient=P01.bam
  check:  samtools quickcheck out/normal_bam/patient=P01.bam
```

Next: [Example directory](../examples.md).
