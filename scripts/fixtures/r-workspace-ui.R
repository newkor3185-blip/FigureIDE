# Owned deterministic UI fixture. Source through the application's existing script editor.
library(ggplot2)
cat("R_WORKSPACE_LOCALE=", Sys.getlocale(), "\n")
df <- data.frame(x = 1:4, y = c(2, 5, 3, 7), category = c("A", "B", "A", "B"))
numeric_value <- 42
character_value <- "UTF-8: \uD55C\uAE00 / caf\u00E9 / \u65E5\u672C\u8A9E"
assign("\uD55C\uAE00_\uBCC0\uC218", "\uD55C\uAE00 \uAC12", envir = .GlobalEnv)
a_very_long_variable_name_for_narrow_workspace_readability <- seq_len(5)
long_metadata <- structure(list(value = 1), class = c(
  "a_very_long_class_name_for_metadata_layout_verification",
  "another_long_class_name_for_narrow_panels", "list"))
plot_A <- ggplot(df, aes(x, y)) + geom_point(color = "#377EB8", size = 3) + theme_classic()
plot_B <- ggplot(df, aes(category, y)) + geom_col(fill = "#4DAF4A") + theme_classic()
local({
  for (i in seq_len(20)) assign(sprintf("fixture_value_%02d", i), i, envir = .GlobalEnv)
})
cat("R_WORKSPACE_UI_FIXTURE_READY\n")
