dnl config.m4 for tokio_sapi extension

PHP_ARG_ENABLE([tokio_sapi],
  [whether to enable tokio_sapi support],
  [AS_HELP_STRING([--enable-tokio_sapi],
    [Enable tokio_sapi support])],
  [no])

if test "$PHP_TOKIO_SAPI" != "no"; then
  AC_DEFINE(HAVE_TOKIO_SAPI, 1, [ Have tokio_sapi support ])

  PHP_NEW_EXTENSION(tokio_sapi, tokio_sapi.c, $ext_shared)
fi
