#!/bin/sh

case $1 in
    start) docker start firebirdsql ;;

    stop) docker stop firebirdsql ;;

    remove) docker rm --volumes --force firebirdsql ;;

    test) echo 'select * from rdb$database;' | isql-fb -bail -quiet -z -user my_user -password my_password 'localhost:test.fdb' && echo 'SUCCESS' || echo "FAILURE" ;;

    create) docker run --detach --publish '3050:3050'  --name 'firebirdsql'  --env 'ISC_PASSWORD=masterkey'  --env 'FIREBIRD_DATABASE=test.fdb' --env 'FIREBIRD_USER=my_user' --env 'FIREBIRD_PASSWORD=my_password' 'jacobalberty/firebird:latest' ;;

    * )	echo '# Usage:\n\t./docker.sh <create | start | stop | remove>\n' ;;
esac

# end of script #
