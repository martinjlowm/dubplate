# Archives in, a named library out.
#
#   zip archives ──unzip──► one derivation per track ──analyse──► wav/ flac/ mp3/
#   loose files  ─────────┘
#
# Every track is its own derivation, so Nix runs as many analyses at once as it
# runs anything else and a failure names the track that failed instead of losing
# the batch. The archives are listed at evaluation time by building a manifest
# and reading it back, which is import-from-derivation: the zip has to be built
# (imported into the store) before the set of tracks is known, because nothing
# in Nix can look inside a zip without unpacking one.
#
# The final derivation has three outputs. Each track lands in the directory of
# its own format, and a WAV lands in all three, encoded on the way. Nothing is
# transcoded from a lossy source, since that would spend CPU to lose more.
{
  lib,
  runCommand,
  unzip,
  flac,
  lame,
  dubplate,
}: let
  # Only these three. A format that reaches the manifest has to have a decode
  # path to WAV for the analyser and a home in one of the outputs; adding one
  # means adding both.
  extensions = ["wav" "flac" "mp3"];
  extensionPattern = "\\.(${lib.concatStringsSep "|" extensions})$";

  # The shared body of a track build. `fetch` leaves exactly one audio file in
  # ./source, and everything after that is the same whichever way it arrived.
  trackScript = fetch: ''
    mkdir -p "$out" "$wav" "$flac" "$mp3" "$analysis" source decoded reports
    ${fetch}

    # A glob rather than `ls`, and no command substitution, because a track is
    # called "Artist,_Other-Title_(Extended_Mix).wav" and nothing stops a shop
    # putting a newline in one. `ls` reports such a name as two lines and `$()`
    # strips the trailing one.
    file=
    for candidate in source/*; do
      if [ -f "$candidate" ]; then
        file=''${candidate#source/}
        break
      fi
    done
    if [ -z "$file" ]; then
      echo "nothing was unpacked" >&2
      exit 1
    fi
    stem="''${file%.*}"
    extension=$(printf '%s' "''${file##*.}" | tr '[:upper:]' '[:lower:]')

    # The analyser reads WAV only. Decoding here rather than in the tool keeps
    # one decoder per format, each the reference implementation for it.
    case "$extension" in
      wav)  cp "source/$file" "decoded/$stem.wav" ;;
      flac) flac --decode --silent --output-name="decoded/$stem.wav" "source/$file" ;;
      mp3)  lame --decode --quiet "source/$file" "decoded/$stem.wav" ;;
      *)    echo "no decoder for .$extension" >&2; exit 1 ;;
    esac

    # The name carries the stem of the decoded file, which is the stem of the
    # original, so the tempo and key are prefixed to the name the track already
    # had.
    dubplate rename "decoded/$stem.wav" --mode print --reports reports > name.txt
    name=$(cat name.txt)
    base="''${name%.wav}"
    echo "$file -> $base"

    # The report is what the exporters read: tempo, key, beat grid and both
    # waveforms, measured once here rather than again per format.
    cp "reports/$stem/report.json" "$analysis/$base.json"
    # The default output records the decision, so one track can be inspected
    # without unpacking the format outputs.
    printf '%s\n%s\n' "$file" "$base" > "$out/name"

    case "$extension" in
      wav)
        cp "source/$file" "$wav/$base.wav"
        flac --best --silent --output-name="$flac/$base.flac" "source/$file"
        lame -V2 --quiet "source/$file" "$mp3/$base.mp3"
        ;;
      flac) cp "source/$file" "$flac/$base.flac" ;;
      mp3)  cp "source/$file" "$mp3/$base.mp3" ;;
    esac
  '';

  trackAttrs = {
    # "out" first, and deliberately tiny: nixpkgs assumes an output by that
    # name exists, and the formats are what anyone actually reads.
    outputs = ["out" "wav" "flac" "mp3" "analysis"];
    nativeBuildInputs = [unzip flac lame dubplate];
  };

  # One entry out of one archive. `unzip -j` drops the path inside the archive,
  # which is what flattens several archives into one collection.
  trackFromArchive = archive: entry:
    runCommand "track-${lib.strings.sanitizeDerivationName (baseNameOf entry)}" trackAttrs
    (trackScript ''
      unzip -j -q "${archive}" ${lib.escapeShellArg entry} -d source
    '');

  trackFromFile = file:
    runCommand "track-${lib.strings.sanitizeDerivationName (baseNameOf file)}" trackAttrs
    (trackScript ''
      cp "${file}" source/${lib.escapeShellArg (baseNameOf file)}
    '');

  # Import from derivation: this builds the archive into the store and reads the
  # list back during evaluation. `nix build` says nothing while it happens, so a
  # first run on a large archive looks like a hang for as long as the copy takes.
  entriesIn = archive:
    lib.filter (line: line != "") (lib.splitString "\n" (builtins.readFile (
      runCommand "${lib.strings.sanitizeDerivationName (baseNameOf archive)}-manifest"
      {nativeBuildInputs = [unzip];}
      ''
        unzip -Z1 "${archive}" | grep -iE '${extensionPattern}' | LC_ALL=C sort > $out
      ''
    )));
in
  {
    # A directory of zip archives, a directory of loose audio files, or both.
    archives ? null,
    audio ? null,
    name ? "music-library",
  }: let
    archiveFiles =
      if archives == null || !builtins.pathExists archives
      then []
      else
        lib.mapAttrsToList (file: _: archives + "/${file}")
        (lib.filterAttrs (file: type: type == "regular" && lib.hasSuffix ".zip" file)
          (builtins.readDir archives));

    looseFiles =
      if audio == null || !builtins.pathExists audio
      then []
      else
        lib.mapAttrsToList (file: _: audio + "/${file}")
        (lib.filterAttrs (
            file: type:
              type == "regular" && builtins.match ".*${extensionPattern}" (lib.toLower file) != null
          )
          (builtins.readDir audio));

    tracks =
      lib.flatten (map (archive: map (trackFromArchive archive) (entriesIn archive)) archiveFiles)
      ++ map trackFromFile looseFiles;
  in
    runCommand name {
      outputs = ["out" "wav" "flac" "mp3" "analysis"];
      passthru = {inherit tracks;};
    } ''
      mkdir -p "$wav" "$flac" "$mp3" "$analysis" "$out"

      # Symlinks, not copies: every track is already in the store under its own
      # derivation, and the output keeps those alive by referring to them.
      link() {
        local source="$1" destination="$2"
        for file in "$source"/*; do
          [ -e "$file" ] || continue
          local target="$destination/$(basename "$file")"
          if [ -e "$target" ]; then
            echo "duplicate name, keeping the first: $(basename "$file")" >&2
            continue
          fi
          ln -s "$file" "$target"
        done
      }

      ${lib.concatMapStringsSep "\n" (track: ''
          link "${track.wav}" "$wav"
          link "${track.flac}" "$flac"
          link "${track.mp3}" "$mp3"
          link "${track.analysis}" "$analysis"
        '')
        tracks}

      # The default output is the three of them side by side, for the common case
      # of wanting to look at the lot.
      ln -s "$wav" "$out/wav"
      ln -s "$flac" "$out/flac"
      ln -s "$mp3" "$out/mp3"
      ln -s "$analysis" "$out/analysis"

      printf 'tracks: %d\n' ${toString (builtins.length tracks)} > "$out/count"
    ''
