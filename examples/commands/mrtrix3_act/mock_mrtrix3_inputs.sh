#!/bin/sh
# Create empty, BIDS-shaped inputs for testing SPIT's MRtrix3 job planning.
# These files are placeholders, not valid MRI images or metadata.
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
data_dir="$script_dir/mrtrix3_mock_data"

mkdir -p "$data_dir/config"
touch "$data_dir/config/source_lut.txt" "$data_dir/config/target_lut.txt"

for session_runs in 01:01:01,02 01:02:01,02 02:01:01,02,03; do
    subject=${session_runs%%:*}
    rest=${session_runs#*:}
    session=${rest%%:*}
    runs=${rest#*:}
    session_dir="$data_dir/sub-$subject/ses-$session"
    stem="sub-${subject}_ses-${session}"

    mkdir -p "$session_dir/dwi" "$session_dir/fmap" "$session_dir/anat"
    touch "$session_dir/fmap/${stem}_dir-PA_epi.nii.gz" \
          "$session_dir/fmap/${stem}_dir-PA_epi.json" \
          "$session_dir/anat/${stem}_T1w.nii.gz"

    old_ifs=$IFS
    IFS=,
    for run in $runs; do
        run_stem="${stem}_run-${run}_dwi"
        touch "$session_dir/dwi/${run_stem}.nii.gz" \
              "$session_dir/dwi/${run_stem}.bvec" \
              "$session_dir/dwi/${run_stem}.bval" \
              "$session_dir/dwi/${run_stem}.json"
    done
    IFS=$old_ifs
done

printf 'Created placeholder inputs in %s\n' "$data_dir"
