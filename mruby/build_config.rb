# mruby build used by Knut's build.rs. Only the library is built; no binaries.
MRuby::Build.new do |conf|
  conf.toolchain
  conf.disable_lock
  conf.enable_debug
  conf.cc.defines << 'MRB_UTF8_STRING'
  conf.cc.flags << '-fPIC'

  conf.gembox 'stdlib'
  conf.gembox 'stdlib-ext'
  conf.gembox 'math'
  conf.gembox 'metaprog'
  conf.gem core: 'mruby-error'
end
