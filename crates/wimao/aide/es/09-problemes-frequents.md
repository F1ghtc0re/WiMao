# Problemas frecuentes

## Un plugin está marcado como «Problema»

El plugin no se carga o falla en Wine durante la prueba de apertura. Lea la explicación bajo su nombre y después:

1. Pruébelo de todos modos en su programa de música: la prueba de WiMao usa otro programa anfitrión, que a veces falla donde su programa de música funciona.
2. Haga clic en **Reparar**: a menudo la causa es un componente que falta.
3. Si el plugin viene de un gestor, compruebe que está bien instalado y activado en él.

Un plugin marcado como **Revisar** no dio un resultado claro (por ejemplo, no terminó de cargarse en 5 minutos): pruébelo en su programa de música.

## «Hay un plugin en ejecución»

El mensaje completo es «Cierre su software de música antes de continuar: hay un plugin en ejecución». WiMao se niega a detener Wine mientras un plugin trabaja en su programa de música, para no hacerlo fallar. No se ha modificado nada: cierre su programa de música y vuelva a empezar.

## Un gestor no se abre

- Espere un poco: el primer inicio de un gestor puede ser lento.
- Si una ventana se queda negra o vacía, ciérrela y haga clic en **Reparar**: la reparación detiene Wine, vuelve a aplicar los ajustes de los gestores y reinicia sus servicios. Después, abra de nuevo el gestor.
- ¿El botón **Abrir** está en gris? Hay otra operación o una sesión de gestor en curso: espere a que termine.

## El navegador no vuelve al gestor después de iniciar sesión

Para Native Access, la vuelta del inicio de sesión pasa por un enlace `native-access://` que WiMao transmite al gestor.

- Deje el gestor abierto durante el inicio de sesión y acepte abrir el enlace si el navegador lo pregunta.
- Si WiMao muestra «Un enlace de inicio de sesión no llegó al gestor», abra de nuevo el enlace desde el navegador.
- Si respondió **Mantener el actual** a la pregunta **Enlace de inicio de sesión**, el enlace va a otro programa. Puede probar a añadir de nuevo el instalador de Native Access con **Añadir un instalador**: WiMao repite entonces la pregunta y usted responde **Usar WiMao**. No hay garantía de que baste.
- Para un plugin que se activa mediante el navegador (Serum 2, por ejemplo), siga la indicación mostrada durante su instalación.

## Disco lleno durante una copia de seguridad

WiMao comprueba el espacio libre antes de empezar e indica cuánto se necesita y cuánto hay disponible. Si el disco se llena o se desconecta durante la escritura, WiMao borra las partes ya escritas: su entorno no se toca. Libere espacio o elija otro disco y vuelva a empezar. Vea [Copia de seguridad y restauración](aide:sauvegarde).

## La importación se rechaza: dos discos diferentes

WiMao solo mueve el entorno dentro de un mismo disco y nunca lo copia: es demasiado grande. No se ha modificado nada y su antiguo entorno sigue funcionando. Vea [Importar un entorno existente](aide:import).

## ¿Sigue bloqueado?

Elija **Copiar el informe** en el menú principal y péguelo en su petición de ayuda. Una instalación que lo ha estropeado todo también se puede deshacer: vea [Historial y vuelta atrás](aide:historique).
