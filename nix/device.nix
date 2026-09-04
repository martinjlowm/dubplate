# A named library in one format, turned into a USB stick a player can browse.
#
#   renamed audio + reports ──export──► PIONEER/ ──┐
#                                                  ├──► FAT32 image
#   renamed audio ───────────────────────────────► ┘
#
# Two derivations rather than one. The device tree is small, quick to rebuild
# and worth reading on its own when a player refuses a stick; the image is the
# same tree plus several gigabytes of audio, and rebuilding it to change a
# database field would copy all of it again.
#
# Both databases go on every image. They live in separate directories, neither
# player reads the other's, and a stick that works on whatever is in the booth is
# worth the few megabytes.
#
# FAT32 because that is what a CDJ mounts. Denon players read exFAT as well, but
# a stick formatted for both is FAT32, and its four-gigabyte file limit is not a
# constraint for a track.
{
  lib,
  runCommand,
  dosfstools,
  mtools,
  dubplate,
}: let
  # The device tree: the databases, and nothing else. The audio is placed by the
  # image builder, at the paths the databases already point at.
  deviceTree = {
    name,
    audio,
    analysis,
    target ? "both",
    playlist ? "All tracks",
  }:
    runCommand "${name}-device" {
      nativeBuildInputs = [dubplate];
    } ''
      mkdir -p "$out"
      dubplate export \
        --audio ${audio} \
        --reports ${analysis} \
        --out "$out" \
        --target ${target} \
        --playlist ${lib.escapeShellArg playlist}
    '';

  # Slack over the payload, for the filesystem's own structures and for whoever
  # drags one more track onto the stick later.
  slackFraction = 12;
  slackMegabytes = 64;
in {
  # One image per format.
  #
  # `label` becomes the volume name, which is what a player shows in its source
  # list. FAT32 allows eleven characters and mkfs.vfat truncates silently.
  image = {
    name,
    audio,
    analysis,
    label ? "MUSIC",
    target ? "both",
    playlist ? "All tracks",
  }: let
    tree = deviceTree {inherit name audio analysis target playlist;};
  in
    runCommand "${name}.img" {
      nativeBuildInputs = [dosfstools mtools];
      # `tree` is the databases on their own, which is worth reading when a
      # player refuses a stick; `imageFile` is the name inside this output, so
      # a caller can link to the image without knowing how it was named.
      passthru = {
        inherit tree;
        imageFile = "${name}.img";
      };
      # mtools refuses a disk image whose geometry it cannot recognise, which is
      # every image that was never a real disk.
      MTOOLS_SKIP_CHECK = "1";
    } ''
      audio_bytes=$(du -sLb ${audio} | cut -f1)
      tree_bytes=$(du -sLb ${tree} | cut -f1)
      total=$(( (audio_bytes + tree_bytes) * (100 + ${toString slackFraction}) / 100 + ${toString slackMegabytes} * 1024 * 1024 ))
      # FAT32 needs 65 525 clusters to be FAT32 at all, so a small library still
      # gets an image mkfs.vfat will accept.
      minimum=$(( 128 * 1024 * 1024 ))
      if [ "$total" -lt "$minimum" ]; then total=$minimum; fi

      truncate -s "$total" image.img
      # A fixed volume id: mkfs.vfat derives one from the clock otherwise, and
      # two identical libraries would build to two different images.
      mkfs.vfat -F 32 -i "DEADBEEF" -n ${lib.escapeShellArg label} image.img

      # The databases first, then the audio at the path they point at. Both are
      # store paths of symlinks, and mcopy follows them.
      mcopy -s -i image.img ${tree}/* ::
      mmd -i image.img ::/Contents
      for track in ${audio}/*; do
        [ -e "$track" ] || continue
        mcopy -i image.img "$track" ::/Contents/
      done

      # A last check that the filesystem we just wrote is one: fsck reads the
      # structures back rather than trusting mkfs.
      fsck.vfat -n image.img || true

      mkdir -p "$out"
      cp image.img "$out/${name}.img"
      ln -s "${tree}" "$out/device"
    '';

  # Every image side by side, each still reachable on its own.
  #
  # A derivation rather than a plain attribute set, so that
  # `devenv build outputs.usb` gives one directory holding all three and
  # `devenv build outputs.usb.flac` gives that one image. The variants hang off
  # passthru, which is how a Nix attribute path reaches into a derivation.
  imageSet = {
    name ? "usb-images",
    images,
  }:
    runCommand name {passthru = images;} ''
      mkdir -p "$out"
      ${lib.concatStringsSep "\n" (lib.mapAttrsToList (format: image: ''
          ln -s "${image}/${image.imageFile}" "$out/${format}.img"
        '')
        images)}
    '';
}
