#!/usr/bin/perl
# Read-only look at a Source 2 demo: frame kinds, net-message ids with counts, and raw dumps of
# chosen message ids. Usage: demscan.pl <file.dem> <outdir> [max_frames] [dump ids, comma separated]
use strict; use warnings;
my ($path, $out, $max, $dump) = @ARGV;
$max ||= 0; my %dump = map { $_ => 1 } split /,/, ($dump // '');
open(my $fh, '<:raw', $path) or die "open: $!";
read($fh, my $hdr, 16);
my ($magic, $info_off, $spawn_off) = unpack('a8 V V', $hdr);
print "magic=", ($magic =~ s/\0//gr), " fileinfo_offset=$info_off spawngroups_offset=$spawn_off\n";

sub rd_varint_fh { my $v = 0; my $s = 0; while (1) { read($fh, my $b, 1) or return undef; $b = ord $b; $v |= ($b & 0x7f) << $s; return $v unless $b & 0x80; $s += 7; } }

sub unsnap {
  my ($in) = @_; my $p = 0; my $len = 0; my $s = 0;
  while (1) { my $b = ord substr($in, $p++, 1); $len |= ($b & 0x7f) << $s; last unless $b & 0x80; $s += 7; }
  my $o = ''; my $n = length $in;
  while ($p < $n) {
    my $t = ord substr($in, $p++, 1); my $k = $t & 3;
    if ($k == 0) { my $l = $t >> 2; if ($l >= 60) { my $nb = $l - 59; $l = 0; for my $i (0 .. $nb - 1) { $l |= ord(substr($in, $p + $i, 1)) << (8 * $i) } $p += $nb; } $l++; $o .= substr($in, $p, $l); $p += $l; next; }
    my ($l, $off);
    if ($k == 1) { $l = (($t >> 2) & 7) + 4; $off = (($t >> 5) << 8) | ord substr($in, $p++, 1); }
    elsif ($k == 2) { $l = ($t >> 2) + 1; $off = unpack('v', substr($in, $p, 2)); $p += 2; }
    else { $l = ($t >> 2) + 1; $off = unpack('V', substr($in, $p, 4)); $p += 4; }
    my $start = length($o) - $off;
    if ($off >= $l) { $o .= substr($o, $start, $l); } else { while ($l > 0) { my $c = $l < $off ? $l : $off; $o .= substr($o, $start, $c); $start += $c; $l -= $c; } }
  }
  return $o;
}

# protobuf: list of [field, wiretype, value]
sub pb { my ($d) = @_; my @f; my $p = 0; my $n = length $d;
  while ($p < $n) { my $k = 0; my $s = 0; while (1) { return undef if $p >= $n; my $b = ord substr($d, $p++, 1); $k |= ($b & 0x7f) << $s; last unless $b & 0x80; $s += 7; return undef if $s > 63 }
    my ($fn, $wt) = ($k >> 3, $k & 7); return undef if $fn == 0;
    if ($wt == 0) { my $v = 0; $s = 0; while (1) { return undef if $p >= $n; my $b = ord substr($d, $p++, 1); $v |= ($b & 0x7f) << $s; last unless $b & 0x80; $s += 7; return undef if $s > 63 } push @f, [$fn, 0, $v]; }
    elsif ($wt == 1) { return undef if $p + 8 > $n; push @f, [$fn, 1, substr($d, $p, 8)]; $p += 8; }
    elsif ($wt == 5) { return undef if $p + 4 > $n; push @f, [$fn, 5, substr($d, $p, 4)]; $p += 4; }
    elsif ($wt == 2) { my $l = 0; $s = 0; while (1) { return undef if $p >= $n; my $b = ord substr($d, $p++, 1); $l |= ($b & 0x7f) << $s; last unless $b & 0x80; $s += 7 } return undef if $p + $l > $n; push @f, [$fn, 2, substr($d, $p, $l)]; $p += $l; }
    else { return undef }
  } return \@f; }

sub show { my ($d, $depth, $ind) = @_; my $f = pb($d); return undef unless $f; my $o = '';
  for my $e (@$f) { my ($fn, $wt, $v) = @$e;
    if ($wt == 0) { $o .= "$ind$fn: $v\n" }
    elsif ($wt == 5) { $o .= sprintf("%s%d: f32 %.3f / u32 %u\n", $ind, $fn, unpack('f<', $v), unpack('V', $v)) }
    elsif ($wt == 1) { $o .= sprintf("%s%d: f64 %.3f / u64 %u\n", $ind, $fn, unpack('d<', $v), unpack('Q<', $v)) }
    else { my $sub = ($depth > 0 && length($v) > 1 && $v !~ /^[\x20-\x7e]+$/) ? show($v, $depth - 1, "$ind  ") : undef;
      if (defined $sub && length $sub) { $o .= "$ind$fn {\n$sub$ind}\n" }
      elsif ($v =~ /^[\x09\x0a\x0d\x20-\x7e]*$/) { $o .= "$ind$fn: \"" . substr($v, 0, 200) . "\"\n" }
      else { $o .= "$ind$fn: bytes[" . length($v) . "] " . unpack('H*', substr($v, 0, 24)) . "\n" } }
  } return $o; }

my (%cmds, %msgs, %first, %last, %bytes, %dumped); my $frames = 0; my $last_tick = 0;
mkdir $out unless -d $out;
while (1) {
  my $cmd = rd_varint_fh(); last unless defined $cmd; my $tick = rd_varint_fh(); my $size = rd_varint_fh();
  $tick = -1 if $tick == 4294967295; read($fh, my $data, $size) == $size or last;
  my $comp = $cmd & 64; $cmd &= ~64; $data = unsnap($data) if $comp;
  $cmds{$cmd}++; $frames++; $last_tick = $tick if $tick > $last_tick;
  if ($cmd == 1 || $cmd == 2) { open(my $o, '>', "$out/cmd$cmd.txt"); print $o show($data, 4, ''); close $o; }
  if ($cmd == 7 || $cmd == 8) {
    my $f = pb($data) or next; my ($pk) = map { $_->[2] } grep { $_->[0] == 3 && $_->[1] == 2 } @$f; next unless defined $pk;
    my $bits = unpack('b*', $pk); my $p = 0; my $nb = length $bits;
    while ($nb - $p >= 8) {
      my $v = oct('0b' . reverse substr($bits, $p, 6)); $p += 6; my $hi = $v & 0x30;
      if ($hi) { my $w = $hi == 16 ? 4 : $hi == 32 ? 8 : 28; $v = ($v & 15) | (oct('0b' . reverse substr($bits, $p, $w)) << 4); $p += $w; }
      my $len = 0; my $s = 0; while (1) { my $b = oct('0b' . reverse substr($bits, $p, 8)); $p += 8; $len |= ($b & 0x7f) << $s; last unless $b & 0x80; $s += 7; }
      last if $p + $len * 8 > $nb;
      $msgs{$v}++; $bytes{$v} += $len; $first{$v} //= $tick; $last{$v} = $tick;
      if ($dump{$v} && ($dumped{$v} // 0) < 400) { $dumped{$v}++; my $m = pack('b*', substr($bits, $p, $len * 8)); open(my $o, '>>', "$out/msg$v.txt"); print $o "=== tick $tick ($len bytes)\n", (show($m, 6, '') // "(not protobuf)\n"); close $o;
        if ($len > 20000) { open(my $r, '>:raw', "$out/msg$v-tick$tick.bin"); print $r $m; close $r; } }
      $p += $len * 8;
    }
  }
  last if $max && $frames >= $max;
}
print "frames=$frames last_tick=$last_tick\n";
print "cmd $_: $cmds{$_}\n" for sort { $a <=> $b } keys %cmds;
printf("msg %d: n=%d bytes=%d first_tick=%d last_tick=%d\n", $_, $msgs{$_}, $bytes{$_}, $first{$_}, $last{$_}) for sort { $a <=> $b } keys %msgs;
