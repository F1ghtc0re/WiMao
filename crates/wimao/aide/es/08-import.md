# Importar un entorno existente

Antes de WiMao, quizá sus plugins ya estaban instalados a mano, en un entorno de Wine guardado en la carpeta `~/.wine-mao`. En lugar de reinstalarlo todo, WiMao puede retomarlo tal cual.

## ¿Cuándo se propone la importación?

En el primer inicio, cuando WiMao todavía no tiene un entorno propio. Si encuentra un `~/.wine-mao` válido, el asistente muestra su tamaño y su número de plugins, y después propone:

- **Importar este entorno (recomendado)**: WiMao retoma sus plugins, sus ajustes y sus licencias;
- **Crear un entorno nuevo**: WiMao empieza de cero y deja `~/.wine-mao` intacto.

## Lo que hace la importación

1. WiMao comprueba que ningún plugin está en ejecución: cierre su programa de música antes de empezar. Si hay un programa de Windows abierto, lo nombra y le pide permiso antes de cerrarlo.
2. Guarda aparte los ajustes del entorno y los de yabridge: una pequeña copia de seguridad, por si acaso.
3. Mueve la carpeta a su propia carpeta de datos. El traslado es instantáneo, incluso con más de 100 GB: no se copia nada.
4. En el lugar antiguo, deja un enlace hacia la nueva ubicación. Los plugins ya puenteados siguen funcionando: su programa de música no nota ninguna diferencia.

Después, la importación aparece en el historial. Sus ajustes personales de yabridge (`~/.config/yabridgectl`) nunca se modifican.

## Importación rechazada: dos discos diferentes

El traslado instantáneo solo es posible dentro de un mismo disco. Si `~/.wine-mao` y la carpeta de datos de WiMao están en dos discos diferentes, WiMao rechaza la importación y explica por qué. Nunca hace una copia: el entorno es demasiado grande. No se modifica nada: su antiguo entorno sigue funcionando donde está. También puede mover la carpeta usted mismo y volver a iniciar WiMao.

## Cancelar la importación

Mientras sea posible, el menú principal propone **Cancelar la importación**. WiMao retira entonces el enlace, devuelve la carpeta a su lugar original en `~/.wine-mao` y actualiza los enlaces de sus plugins con su propia configuración de yabridge. No se borra nada.

La cancelación se rechaza si `~/.wine-mao` ya no es el enlace creado por la importación: WiMao prefiere no sobrescribir nada. Como la importación, exige que su programa de música esté cerrado.

Después de la cancelación, WiMao ya no tiene entorno: vuelve el asistente del primer inicio. Los puntos de restauración creados hasta entonces ya no se proponen en el historial: describen el entorno devuelto a `~/.wine-mao`. Vea [Primeros pasos](aide:premiers-pas) y [Problemas frecuentes](aide:problemes).
