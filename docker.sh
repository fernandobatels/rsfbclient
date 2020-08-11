#!/bin/sh

set -e

case $2 in
    ""|v3|latest) FIREBIRD_VERSION='latest' ;;
    v2) FIREBIRD_VERSION='2.5-ss' ;;
    v4) FIREBIRD_VERSION='4.0' ;;
    *) echo '# Firebird version must be: v2 v3 or v4' && exit 1 ;;
esac

case $1 in
    start) docker start firebirdsql ;;

    stop) docker stop firebirdsql ;;

    remove) docker rm --volumes --force firebirdsql ;;

    test) echo 'select * from rdb$database;' | isql-fb -bail -quiet -z -user my_user -password my_password 'localhost:test.fdb' && echo 'SUCCESS' || echo "FAILURE" ;;

    create) docker run --detach --publish '3050:3050'  --name 'firebirdsql'  --env 'ISC_PASSWORD=masterkey'  --env 'FIREBIRD_DATABASE=test.fdb' --env 'FIREBIRD_USER=my_user' --env 'FIREBIRD_PASSWORD=my_password' "jacobalberty/firebird:${FIREBIRD_VERSION}" ;;

    * )	echo '# Usage:\n\t./docker.sh <create | start | stop | remove>\n' ;;
esac

# end of script #
