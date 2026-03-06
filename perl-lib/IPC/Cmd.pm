package IPC::Cmd;
use strict;
use warnings;
our $VERSION = '1.02';

sub can_run {
    my ($cmd) = @_;
    return undef unless defined $cmd;
    
    for my $dir (split /:/, $ENV{PATH}) {
        my $full = "$dir/$cmd";
        return $full if -x $full;
    }
    return undef;
}

sub run {
    my ($cmd, @args) = @_;
    system($cmd, @args);
    return $? == 0;
}

1;
