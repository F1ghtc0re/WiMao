# Copia de seguridad y restauración

Una copia de seguridad copia todo el entorno de Wine en un único archivo: todo lo instalado en ese entorno, es decir, sus plugins, sus ajustes, sus licencias y los sonidos que hayan dejado allí. Lo que esté en otra parte de sus discos no forma parte de él. Si su disco falla o una actualización lo estropea todo, recupera su instalación sin volver a descargarlo todo.

## Hacer una copia de seguridad

1. Cierre su programa de música y conecte un disco externo.
2. Haga clic en **Guardar copia**, arriba a la derecha (el icono en forma de disco).
3. Elija la carpeta de destino, preferiblemente en el disco externo.
4. Espere hasta el final: tarda varios minutos, según el tamaño de sus plugins y de sus sonidos.

WiMao comprueba primero que hay espacio libre suficiente. Guarda la copia en una carpeta `WiMao-sauvegarde`, en forma de archivo con fecha, por ejemplo `wimao-2026-11-01-14h30.tar.zst`, acompañado de un archivo `.sha256`. Este archivo contiene las sumas de comprobación que permiten verificar que el archivo está intacto. Conserve siempre los dos juntos.

Al final, un mensaje indica dónde se ha guardado la copia. El botón **Mostrar** abre la carpeta.

## Conviene saber

- En un disco formateado en FAT32, que no admite archivos de más de 4 GB, el archivo se divide en partes de 3,9 GB (`.part000`, `.part001`…). Conserve todas las partes.
- Tres carpetas de cada usuario de Windows del entorno no se copian: la carpeta temporal, la carpeta **Downloads** (los instaladores descargados) y la caché de DXVK. No hacen falta o se vuelven a crear solas.
- Si la escritura falla (disco desconectado o lleno), WiMao borra las partes ya escritas. Nunca deja una copia incompleta que parezca completa.

## Restaurar una copia de seguridad

1. Cierre su programa de música y conecte el disco que contiene la copia.
2. En el menú principal, elija **Restaurar una copia de seguridad…**.
3. Seleccione el archivo `.sha256` de la copia.
4. WiMao muestra la fecha y el tamaño del archivo. Confirme.

WiMao comprueba primero las sumas de comprobación de todas las partes. Si una es incorrecta, se detiene sin tocar nada. Si no, extrae el archivo junto al entorno actual y después intercambia los dos. Su entorno actual no se borra: se conserva con el nombre `prefixe.ancien-<fecha>`, en la carpeta de datos de WiMao. Podrá borrarlo usted mismo cuando esté tranquilo.

Al final, un mensaje confirma **Copia de seguridad restaurada** y se vuelve a leer la lista de plugins.

Después de una restauración, los antiguos puntos de restauración ya no corresponden al entorno: el historial ya no los propone. Vea [Historial y vuelta atrás](aide:historique).

¿Un problema de espacio? Vea [Problemas frecuentes](aide:problemes).
