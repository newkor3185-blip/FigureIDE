//! Persistent local R process used by the scientific workspace.

use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

const BRIDGE: &str = r#"
local({
decode_hex <- function(value) {
  if (nchar(value, type = "bytes") == 0) return("")
  positions <- seq.int(1, nchar(value, type = "bytes"), by = 2)
  bytes <- as.raw(strtoi(substring(value, positions, positions + 1), base = 16L))
  decoded <- rawToChar(bytes)
  Encoding(decoded) <- "UTF-8"
  decoded
}
encode_hex <- function(value) {
  bytes <- charToRaw(enc2utf8(value))
  paste(sprintf("%02x", as.integer(bytes)), collapse = "")
}
metadata_size_cache <- new.env(parent = emptyenv())
format_object_size <- function(name, object, signature) {
  previous <- get0(name, envir = metadata_size_cache, inherits = FALSE)
  if (!is.null(previous) && identical(previous$signature, signature)) return(previous$text)
  bytes <- tryCatch(as.numeric(utils::object.size(object)), error = function(condition) NA_real_)
  text <- if (!is.finite(bytes)) "" else if (bytes >= 1024^3) sprintf("%.1f GB", bytes / 1024^3)
    else if (bytes >= 1024^2) sprintf("%.1f MB", bytes / 1024^2)
    else sprintf("%.1f KB", bytes / 1024)
  assign(name, list(signature = signature, text = text), envir = metadata_size_cache)
  text
}
plot_kind <- function(object) {
  if (inherits(object, "ggplot")) return("ggplot")
  if (inherits(object, "pheatmap") && is.list(object) && inherits(object[["gtable"]], "gtable")) return("pheatmap")
  ""
}
describe_object <- function(name, object, classes, dimensions) {
  class_name <- if (length(classes)) classes[[1L]] else typeof(object)
  tabular <- is.data.frame(object) || is.matrix(object) ||
    any(classes %in% c("data.table", "DESeqDataSet", "DESeqTransform", "SummarizedExperiment", "SingleCellExperiment")) ||
    (isS4(object) && length(dimensions) >= 2L)
  category <- if (is.function(object)) "Functions" else if (tabular) "Data" else "Values"
  element_count <- if (length(dimensions) >= 1L && isS4(object)) as.double(dimensions[[1L]]) else as.double(length(object))
  signature <- paste(class_name, typeof(object), length(object), paste(dimensions, collapse = "x"), sep = ":")
  size <- if (isS4(object) || element_count > 5000) format_object_size(name, object, signature) else ""
  size_suffix <- if (nzchar(size)) paste0(", ", size) else ""
  if (category == "Functions") {
    arguments <- tryCatch(names(formals(object)), error = function(condition) character())
    arguments <- head(arguments, 6L)
    description <- paste0("function(", paste(arguments, collapse = ", "), if (length(formals(object)) > 6L) ", ..." else "", ")")
  } else if (category == "Data" && is.data.frame(object) && length(dimensions) >= 2L) {
    description <- sprintf("%s obs. of %s variables%s", dimensions[[1L]], dimensions[[2L]], size_suffix)
  } else if (category == "Data" && is.matrix(object) && length(dimensions) >= 2L) {
    description <- sprintf("%s x %s matrix%s", dimensions[[1L]], dimensions[[2L]], size_suffix)
  } else if (category == "Data") {
    description <- sprintf("%s%s (%s elements%s)", if (element_count > 5000) "Large " else "", class_name, format(element_count, scientific = FALSE), size_suffix)
  } else if (plot_kind(object) == "ggplot") {
    description <- "ggplot2::ggplot"
  } else if (plot_kind(object) == "pheatmap") {
    description <- "pheatmap::pheatmap"
  } else if (is.atomic(object) && is.null(dimensions)) {
    type <- switch(typeof(object), character = "chr", integer = "int", double = "num", logical = "logi", complex = "cplx", raw = "raw", typeof(object))
    if (is.factor(object)) type <- "factor"
    values <- if (length(object)) vapply(head(seq_along(object), 4L), function(index) {
      value <- as.character(object[[index]])
      if (is.na(value)) return("NA")
      value <- gsub("[\r\n\t]", " ", value)
      value <- substr(value, 1L, 45L)
      if (is.character(object)) paste0('"', value, '"') else value
    }, character(1L)) else character()
    range <- if (length(object) > 1L) paste0(" [1:", length(object), "]") else ""
    description <- paste0(type, range, if (length(values)) paste0(" ", paste(values, collapse = " ")) else "", if (length(object) > 4L) " ..." else "", size_suffix)
  } else {
    description <- paste0(class_name, " (", length(object), " elements", size_suffix, ")")
  }
  description <- gsub("[\r\n\t]", " ", description)
  list(category = category, description = substr(description, 1L, 180L))
}
evaluate_source <- function(source, output_path) {
  # Parse the complete submission before evaluating anything. Runtime errors still preserve
  # earlier expressions, matching R's sequential evaluation in the shared global environment.
  expressions <- parse(text = source, keep.source = TRUE)
  output_connection <- file(output_path, open = "wt", encoding = "UTF-8")
  original_sink_depth <- sink.number(type = "output")
  sink(output_connection, type = "output")
  on.exit({
    while (sink.number(type = "output") > original_sink_depth) {
      previous_depth <- sink.number(type = "output")
      try(sink(type = "output"), silent = TRUE)
      if (sink.number(type = "output") >= previous_depth) break
    }
    try(close(output_connection), silent = TRUE)
  }, add = TRUE)
  for (expression in expressions) {
    withCallingHandlers(
      {
        evaluated <- withVisible(base::eval(expression, envir = .GlobalEnv))
        if (isTRUE(evaluated$visible)) base::print(evaluated$value)
        evaluated
      },
      warning = function(condition) {
        warning_text <- sub("[\\r\\n]+$", "", conditionMessage(condition))
        captured_warnings <<- c(captured_warnings, warning_text)
        flush(output_connection)
        writeLines(paste0("Warning: ", warning_text), output_connection, useBytes = TRUE)
        flush(output_connection)
        invokeRestart("muffleWarning")
      },
      message = function(condition) {
        message_text <- sub("[\\r\\n]+$", "", conditionMessage(condition))
        captured_messages <<- c(captured_messages, message_text)
        flush(output_connection)
        writeLines(paste0("Info: ", message_text), output_connection, useBytes = TRUE)
        flush(output_connection)
        invokeRestart("muffleMessage")
      }
    )
    flush(output_connection)
  }
}
args <- commandArgs(trailingOnly = TRUE)
request_path <- args[[1L]]
response_path <- args[[2L]]
repeat {
  if (!file.exists(request_path)) {
    Sys.sleep(0.02)
    next
  }
  request <- readLines(request_path, n = 1L, warn = FALSE)
  unlink(request_path)
  if (length(request) == 0L) next
  fields <- strsplit(request, "\t", fixed = TRUE)[[1L]]
  if (length(fields) != 3L) next
  if (fields[[2L]] == "STOP") break
  request_id <- fields[[1L]]
  mode <- fields[[2L]]
  payload <- if (fields[[3L]] == "-") "" else decode_hex(fields[[3L]])
  captured_warnings <- character()
  captured_messages <- character()
  captured_error <- ""
  execution_mode <- mode %in% c("EXEC", "EXEC_STREAM")
  stream_path <- file.path(dirname(response_path), "console.log")
  if (execution_mode) unlink(stream_path)
  captured_output <- if (execution_mode) {
    tryCatch({
      evaluate_source(payload, stream_path)
      character()
    }, error = function(condition) {
      captured_error <<- conditionMessage(condition)
      character()
    })
  } else tryCatch(capture.output({
      if (mode == "PARSE") {
      tryCatch({
        parse(text = payload, keep.source = TRUE)
        cat("COMPLETE")
      }, error = function(condition) {
        message <- conditionMessage(condition)
        if (grepl("unexpected end of input", message, fixed = TRUE) || grepl("unexpected INCOMPLETE_STRING", message, fixed = TRUE)) {
          cat("INCOMPLETE")
        } else {
          captured_error <<- message
        }
      })
      } else if (mode == "VERIFY_PAYLOAD") {
      cat(encode_hex(payload))
      } else if (mode == "RENDER") {
      parts <- strsplit(payload, "\t", fixed = TRUE)[[1L]]
      if (!(length(parts) %in% c(4L, 5L))) stop("Invalid plot render request")
      plot_name <- decode_hex(parts[[1L]])
      width_mm <- suppressWarnings(as.numeric(parts[[2L]]))
      height_mm <- suppressWarnings(as.numeric(parts[[3L]]))
      small_figure <- identical(parts[[4L]], "small")
      graph_fields <- if (length(parts) == 5L && parts[[5L]] != "-") strsplit(parts[[5L]], "|", fixed = TRUE)[[1L]] else rep("-", 23L)
      if (length(graph_fields) == 17L) graph_fields <- c(graph_fields, rep("-", 6L))
      if (length(graph_fields) < 23L) stop("Invalid Graph style request")
      graph_extra <- list()
      if (length(graph_fields) > 23L) {
        for (field in graph_fields[24:length(graph_fields)]) {
          pair <- strsplit(field, "=", fixed = TRUE)[[1L]]
          if (length(pair) != 2L || !grepl("^[a-z][a-z0-9.]*$", pair[[1L]]) || !is.null(graph_extra[[pair[[1L]]]])) stop("Invalid Graph extension field")
          graph_extra[[pair[[1L]]]] <- pair[[2L]]
        }
        graph_fields <- graph_fields[1:23]
      }
      text_prefixes <- c("plot.title", "plot.subtitle", "plot.caption", "axis.title.x", "axis.title.y", "axis.text.x", "axis.text.y", "legend.title", "legend.text", "strip.text")
      allowed_graph_extra <- c(
        as.vector(outer(text_prefixes, c("family", "face", "colour", "angle", "hjust", "vjust"), paste, sep = ".")),
        "plot.subtitle.size", "plot.caption.size", "strip.text.size",
        "plot.subtitle.visible", "plot.caption.visible", "strip.text.visible",
        "axis.line.x.colour", "axis.line.y.colour",
        "axis.ticks.x.visible", "axis.ticks.y.visible", "axis.ticks.x.colour", "axis.ticks.y.colour",
        "axis.ticks.x.width", "axis.ticks.y.width", "axis.ticks.x.length", "axis.ticks.y.length",
        "panel.grid.major.x.visible", "panel.grid.major.y.visible", "panel.grid.minor.x.visible", "panel.grid.minor.y.visible",
        "panel.grid.major.colour", "panel.grid.minor.colour", "panel.grid.major.width", "panel.grid.minor.width",
        "panel.background.fill", "plot.background.fill", "panel.border.visible", "panel.border.colour", "panel.border.width",
        "legend.direction", "legend.justification.x", "legend.justification.y", "legend.key.width", "legend.key.height",
        "legend.key.fill", "legend.background.fill", "legend.spacing.x", "legend.spacing.y",
        paste0("legend.margin.", c("top", "right", "bottom", "left")),
        paste0("plot.margin.", c("top", "right", "bottom", "left")),
        "plot.title.position", "strip.background.fill", "panel.spacing"
      )
      if (length(setdiff(names(graph_extra), allowed_graph_extra)) > 0L) stop("Unknown Graph extension field")
      graph_number <- function(index, upper = 100) {
        value <- graph_fields[[index]]
        if (value == "-") return(NULL)
        parsed <- suppressWarnings(as.numeric(value))
        if (length(parsed) != 1L || !is.finite(parsed) || parsed < 0 || parsed > upper) stop("Invalid Graph numeric value")
        parsed
      }
      graph_flag <- function(index) {
        value <- graph_fields[[index]]
        if (value == "-") return(NULL)
        if (!(value %in% c("0", "1"))) stop("Invalid Graph visibility value")
        value == "1"
      }
      extra_value <- function(key) graph_extra[[key]]
      extra_number <- function(key, lower = 0, upper = 100) {
        value <- extra_value(key)
        if (is.null(value)) return(NULL)
        number <- suppressWarnings(as.numeric(value))
        if (length(number) != 1L || !is.finite(number) || number < lower || number > upper) stop(sprintf("Invalid Graph numeric value: %s", key))
        number
      }
      extra_flag <- function(key) {
        value <- extra_value(key)
        if (is.null(value)) return(NULL)
        if (!(value %in% c("0", "1"))) stop(sprintf("Invalid Graph visibility value: %s", key))
        value == "1"
      }
      extra_string <- function(key) {
        value <- extra_value(key)
        if (is.null(value)) return(NULL)
        if (!startsWith(value, "h")) stop(sprintf("Invalid Graph text value: %s", key))
        decode_hex(substring(value, 2L))
      }
      extra_colour <- function(key) {
        value <- extra_string(key)
        if (is.null(value)) return(NULL)
        tryCatch(grDevices::col2rgb(value), error = function(condition) stop(sprintf("Invalid R colour for %s: %s", key, value)))
        value
      }
      colour_keys <- grep("(colour|fill)$", names(graph_extra), value = TRUE)
      for (key in colour_keys) extra_colour(key)
      if (!is.finite(width_mm) || !is.finite(height_mm) || width_mm <= 0 || height_mm <= 0) stop("Plot dimensions must be finite and positive")
      render_plot <- function() {
        plot_object <- get(plot_name, envir = .GlobalEnv, inherits = FALSE)
        # ggplot objects can contain mutable environments; deep-copy before inspection, theme and print.
        if (inherits(plot_object, "ggplot")) plot_object <- unserialize(serialize(plot_object, NULL))
        kind <- plot_kind(plot_object)
        if (!nzchar(kind)) stop("The selected object is not a supported plot")
        if (kind != "ggplot" && (any(graph_fields != "-") || length(graph_extra) > 0L)) stop("Graph styling is available for ggplot objects only")
        plot_path <- file.path(dirname(response_path), "plot.svg")
        device <- if (requireNamespace("svglite", quietly = TRUE)) svglite::svglite else grDevices::svg
        device(plot_path, width = width_mm / 25.4, height = height_mm / 25.4, bg = "transparent")
        device_id <- grDevices::dev.cur()
        on.exit(if (!is.null(grDevices::dev.list()) && device_id %in% grDevices::dev.list()) try(grDevices::dev.off(which = device_id), silent = TRUE), add = TRUE)
        if (kind == "ggplot") {
          # Render a temporary derivative; keep the named object and its explicit theme intact.
          if (small_figure) {
            base_size <- tryCatch(plot_object$theme$text$size, error = function(condition) NULL)
            if (is.null(base_size) || (is.numeric(base_size) && length(base_size) == 1L && isTRUE(all.equal(as.numeric(base_size), 11)))) {
              plot_object <- plot_object + ggplot2::theme(text = ggplot2::element_text(size = 8))
            }
          }
          style <- list()
          line_element <- function(width = NULL, visible = NULL, colour = NULL) {
            if (identical(visible, FALSE)) return(ggplot2::element_blank())
            args <- Filter(Negate(is.null), list(linewidth = width, colour = colour))
            if (length(args) == 0L && is.null(visible)) return(NULL)
            do.call(ggplot2::element_line, args)
          }
          x_line <- line_element(graph_number(1L, 20), graph_flag(3L), extra_colour("axis.line.x.colour"))
          y_line <- line_element(graph_number(2L, 20), graph_flag(4L), extra_colour("axis.line.y.colour"))
          if (!is.null(x_line)) style$axis.line.x <- x_line
          if (!is.null(y_line)) style$axis.line.y <- y_line
          for (direction in c("x", "y")) {
            prefix <- paste0("axis.ticks.", direction)
            ticks <- line_element(extra_number(paste0(prefix, ".width"), 0.01, 20), extra_flag(paste0(prefix, ".visible")), extra_colour(paste0(prefix, ".colour")))
            if (!is.null(ticks)) style[[prefix]] <- ticks
            length_mm <- extra_number(paste0(prefix, ".length"))
            if (!is.null(length_mm)) style[[paste0("axis.ticks.length.", direction)]] <- grid::unit(length_mm, "mm")
          }
          major_grid <- graph_flag(5L)
          if (!is.null(major_grid)) style$panel.grid.major <- if (major_grid) ggplot2::element_line(colour = "grey85") else ggplot2::element_blank()
          for (grade in c("major", "minor")) {
            prefix <- paste0("panel.grid.", grade)
            grid_line <- line_element(extra_number(paste0(prefix, ".width"), 0.01, 20), NULL, extra_colour(paste0(prefix, ".colour")))
            if (!is.null(grid_line) && !(grade == "major" && identical(major_grid, FALSE))) style[[prefix]] <- grid_line
            for (direction in c("x", "y")) {
              visibility <- extra_flag(paste0(prefix, ".", direction, ".visible"))
              if (!is.null(visibility)) style[[paste0(prefix, ".", direction)]] <- line_element(NULL, visibility)
            }
          }
          text_element <- function(prefix, size_index = NULL, visible_index = NULL) {
            visible <- if (is.null(visible_index)) extra_flag(paste0(prefix, ".visible")) else graph_flag(visible_index)
            if (identical(visible, FALSE)) return(ggplot2::element_blank())
            size <- if (is.null(size_index)) extra_number(paste0(prefix, ".size"), 0.1, 100) else graph_number(size_index)
            family <- extra_string(paste0(prefix, ".family"))
            face <- extra_value(paste0(prefix, ".face"))
            if (!is.null(face) && !(face %in% c("plain", "bold", "italic", "bold.italic"))) stop("Invalid Graph font face")
            colour <- extra_colour(paste0(prefix, ".colour"))
            angle <- extra_number(paste0(prefix, ".angle"), -360, 360)
            hjust <- extra_number(paste0(prefix, ".hjust"), 0, 1)
            vjust <- extra_number(paste0(prefix, ".vjust"), 0, 1)
            args <- Filter(Negate(is.null), list(size = size, family = family, face = face, colour = colour, angle = angle, hjust = hjust, vjust = vjust))
            if (length(args) == 0L && is.null(visible)) return(NULL)
            do.call(ggplot2::element_text, args)
          }
          for (entry in list(
            list("axis.text.x", 6L, 21L), list("axis.text.y", 7L, 22L), list("plot.title", 8L, 18L),
            list("axis.title.x", 9L, 19L), list("axis.title.y", 10L, 20L),
            list("legend.title", 11L, 14L), list("legend.text", 12L, 23L)
          )) {
            element <- text_element(entry[[1L]], entry[[2L]], entry[[3L]])
            if (!is.null(element)) style[[entry[[1L]]]] <- element
          }
          for (prefix in c("plot.subtitle", "plot.caption", "strip.text")) {
            element <- text_element(prefix)
            if (!is.null(element)) style[[prefix]] <- element
          }
          title_position <- extra_value("plot.title.position")
          if (!is.null(title_position)) {
            if (!(title_position %in% c("panel", "plot"))) stop("Invalid plot title position")
            style$plot.title.position <- title_position
          }
          rectangle <- function(fill = NULL, colour = NULL, linewidth = NULL, visible = NULL) {
            if (identical(visible, FALSE)) return(ggplot2::element_blank())
            args <- Filter(Negate(is.null), list(fill = fill, colour = colour, linewidth = linewidth))
            if (length(args) == 0L && is.null(visible)) return(NULL)
            do.call(ggplot2::element_rect, args)
          }
          style$panel.background <- rectangle(fill = extra_colour("panel.background.fill"))
          style$plot.background <- rectangle(fill = extra_colour("plot.background.fill"))
          border_colour <- extra_colour("panel.border.colour")
          border_width <- extra_number("panel.border.width", 0.01, 20)
          border_visible <- extra_flag("panel.border.visible")
          if (!is.null(border_colour) || !is.null(border_width) || !is.null(border_visible)) {
            style$panel.border <- rectangle(fill = NA, colour = border_colour, linewidth = border_width, visible = border_visible)
          }
          style$strip.background <- rectangle(fill = extra_colour("strip.background.fill"))
          style$legend.key <- rectangle(fill = extra_colour("legend.key.fill"))
          style$legend.background <- rectangle(fill = extra_colour("legend.background.fill"))
          style <- Filter(Negate(is.null), style)
          legend_position <- graph_fields[[13L]]
          if (legend_position != "-") {
            if (!(legend_position %in% c("right", "left", "top", "bottom", "inside", "none"))) stop("Invalid legend position")
            if (legend_position == "inside") {
              if (!("legend.position.inside" %in% names(formals(ggplot2::theme)))) stop("This ggplot2 version does not support inside legend positioning")
              style$legend.position.inside <- c(if (is.null(graph_number(16L, 1))) 0.95 else graph_number(16L, 1),
                                                if (is.null(graph_number(17L, 1))) 0.95 else graph_number(17L, 1))
            }
            style$legend.position <- legend_position
          }
          key_size <- graph_number(15L)
          if (!is.null(key_size)) style$legend.key.size <- grid::unit(key_size, "mm")
          for (side in c("width", "height")) {
            key_mm <- extra_number(paste0("legend.key.", side))
            if (!is.null(key_mm)) style[[paste0("legend.key.", side)]] <- grid::unit(key_mm, "mm")
          }
          direction <- extra_value("legend.direction")
          if (!is.null(direction)) {
            if (!(direction %in% c("horizontal", "vertical"))) stop("Invalid legend direction")
            style$legend.direction <- direction
          }
          justification_x <- extra_number("legend.justification.x", 0, 1)
          justification_y <- extra_number("legend.justification.y", 0, 1)
          if (!is.null(justification_x) || !is.null(justification_y)) {
            style$legend.justification <- c(if (is.null(justification_x)) 0.5 else justification_x, if (is.null(justification_y)) 0.5 else justification_y)
          }
          for (direction in c("x", "y")) {
            spacing <- extra_number(paste0("legend.spacing.", direction))
            if (!is.null(spacing)) style[[paste0("legend.spacing.", direction)]] <- grid::unit(spacing, "mm")
          }
          for (prefix in c("legend.margin", "plot.margin")) {
            sides <- c("top", "right", "bottom", "left")
            values <- lapply(sides, function(side) extra_number(paste0(prefix, ".", side), -100, 100))
            if (any(!vapply(values, is.null, logical(1L)))) {
              values <- vapply(values, function(value) if (is.null(value)) 0 else value, numeric(1L))
              style[[prefix]] <- ggplot2::margin(t = values[[1L]], r = values[[2L]], b = values[[3L]], l = values[[4L]], unit = "mm")
            }
          }
          panel_spacing <- extra_number("panel.spacing")
          if (!is.null(panel_spacing)) style$panel.spacing <- grid::unit(panel_spacing, "mm")
          unsupported <- setdiff(names(style), names(formals(ggplot2::theme)))
          if (length(unsupported) > 0L) stop(sprintf("Installed ggplot2 does not support Graph setting(s): %s", paste(unsupported, collapse = ", ")))
          if (length(style) > 0L) plot_object <- plot_object + do.call(ggplot2::theme, style)
          base::print(plot_object)
        }
        else {
          grid::grid.newpage()
          grid::grid.draw(plot_object[["gtable"]])
        }
      }
      render_plot()
      } else if (mode == "REMOVE") {
      if (!exists(payload, envir = .GlobalEnv, inherits = FALSE)) stop("Object no longer exists")
      rm(list = payload, envir = .GlobalEnv)
    } else if (mode == "CLEAR") {
      rm(list = ls(envir = .GlobalEnv, all.names = TRUE), envir = .GlobalEnv)
    } else if (mode == "DATA") {
      parts <- strsplit(payload, "\t", fixed = TRUE)[[1L]]
      if (length(parts) != 3L) stop("Invalid data page request")
      object_name <- parts[[1L]]
      start <- suppressWarnings(as.integer(parts[[2L]]))
      page_size <- suppressWarnings(as.integer(parts[[3L]]))
      if (is.na(start) || is.na(page_size) || start < 0L || page_size < 1L || page_size > 200L) stop("Invalid data page range")
      if (!exists(object_name, envir = .GlobalEnv, inherits = FALSE)) stop(sprintf("Object '%s' not found in R environment", object_name))
      object <- get(object_name, envir = .GlobalEnv, inherits = FALSE)
      if (is.matrix(object)) {
        object <- as.data.frame(object)
      } else if (is.factor(object) || (is.vector(object) && !is.list(object))) {
        object <- data.frame(Value = object)
      } else if (!is.data.frame(object)) {
        df_attempt <- tryCatch(as.data.frame(object), error = function(e) NULL)
        if (!is.null(df_attempt)) {
          object <- df_attempt
        } else {
          stop(sprintf("Object '%s' of class '%s' cannot be viewed as a table", object_name, paste(class(object), collapse = "/")))
        }
      }
      encode <- function(value) encode_hex(enc2utf8(as.character(value)))
      col_names <- names(object)
      if (is.null(col_names) || length(col_names) != length(object)) {
        col_names <- paste0("V", seq_along(object))
        names(object) <- col_names
      }
      column_line <- paste(vapply(seq_along(object), function(i) {
        col_type <- class(object[[i]])[[1L]]
        paste(encode(col_names[[i]]), encode(col_type), sep = ":")
      }, character(1L)), collapse = "\t")
      total <- nrow(object)
      if (is.null(total)) total <- 0L
      if (total > 0L && start < total) {
        indices <- seq.int(start + 1L, min(total, start + page_size))
        rows <- vapply(indices, function(row) paste(vapply(seq_along(object), function(column) {
          value <- object[[column]][[row]]
          if (length(value) == 0L || is.na(value)) "NA" else {
            text <- as.character(value[[1L]])
            if (nchar(text, type = "bytes") > 512L) paste0(substr(text, 1L, 509L), "...") else text
          }
        }, character(1L)), collapse = "\t"), character(1L))
      } else rows <- character()
      encoded_rows <- vapply(rows, function(row) {
        cells <- strsplit(row, "\t", fixed = TRUE)[[1L]]
        paste(vapply(cells, encode, character(1L)), collapse = "\t")
      }, character(1L))
      cat(paste(c(paste("DATA", total, column_line, sep = "|"), encoded_rows), collapse = "\n"))
    } else if (mode == "EXPR") {
      parts <- strsplit(payload, "\n", fixed = TRUE)[[1L]]
      cursor_line <- suppressWarnings(as.integer(parts[[1L]]))
      source <- paste(parts[-1L], collapse = "\n")
      parsed <- parse(text = source, keep.source = TRUE)
      refs <- attr(parsed, "srcref")
      chosen <- which(vapply(refs, function(ref) ref[[1L]] <= cursor_line && ref[[3L]] >= cursor_line, logical(1L)))[1L]
      if (is.na(chosen)) chosen <- which(vapply(refs, function(ref) ref[[1L]] > cursor_line, logical(1L)))[1L]
      if (is.na(chosen)) stop("No complete R expression at or after the cursor")
      ref <- refs[[chosen]]
      next_line <- if (chosen < length(refs)) refs[[chosen + 1L]][[1L]] else 0L
      lines <- strsplit(source, "\n", fixed = TRUE)[[1L]]
      cat(ref[[1L]], "\t", ref[[3L]], "\t", next_line, "\n", paste(lines[ref[[1L]]:ref[[3L]]], collapse = "\n"), sep = "")
    } else if (mode == "DEFS") {
      parsed <- parse(text = payload, keep.source = TRUE)
      refs <- attr(parsed, "srcref")
      definitions <- vapply(seq_along(parsed), function(index) {
        expression <- parsed[[index]]
        if (!is.call(expression)) return("")
        operator <- as.character(expression[[1L]])
        target <- if (operator %in% c("<-", "=", "<<-") && length(expression) >= 3L) expression[[2L]]
          else if (operator %in% c("->", "->>") && length(expression) >= 3L) expression[[3L]] else NULL
        if (!is.symbol(target)) return("")
        paste(encode_hex(as.character(target)), refs[[index]][[1L]], sep = "\t")
      }, character(1L))
      cat(paste(definitions[nzchar(definitions)], collapse = "\n"))
    } else if (mode == "INSPECT") {
      # Object metadata is returned in the protocol's structured metadata field.
      invisible(NULL)
    } else if (mode == "COMPLETE_META") {
      parts <- strsplit(payload, "\t", fixed = TRUE)[[1L]]
      meta_kind <- parts[[1L]]
      if (meta_kind == "MEMBER" && length(parts) >= 2L) {
        obj_name <- parts[[2L]]
        if (exists(obj_name, envir = .GlobalEnv, inherits = FALSE)) {
          obj <- get(obj_name, envir = .GlobalEnv, inherits = FALSE)
          if (is.data.frame(obj) || is.list(obj) || !is.null(names(obj))) {
            nms <- head(names(obj), 300L)
            entries <- vapply(nms, function(col_name) {
              elem <- tryCatch(obj[[col_name]], error = function(e) NULL)
              type_name <- if (is.null(elem)) "unknown" else class(elem)[[1L]]
              paste(encode_hex(col_name), encode_hex(type_name), sep = "\t")
            }, character(1L))
            cat(paste(entries, collapse = "\n"))
          }
        }
      } else if (meta_kind == "NS" && length(parts) >= 3L) {
        pkg_name <- parts[[2L]]
        internal_flag <- identical(parts[[3L]], "1")
        if (requireNamespace(pkg_name, quietly = TRUE)) {
          ns <- asNamespace(pkg_name)
          syms <- if (internal_flag) ls(ns, all.names = TRUE) else getNamespaceExports(ns)
          syms <- head(sort(syms), 300L)
          entries <- vapply(syms, function(sym) {
            val <- tryCatch(get0(sym, envir = ns, inherits = FALSE), error = function(e) NULL)
            is_fn <- is.function(val)
            kind_str <- if (is_fn) "function" else "variable"
            cls_str <- if (is.null(val)) "symbol" else class(val)[[1L]]
            sig_str <- if (is_fn) {
              args <- tryCatch(names(formals(val)), error = function(e) character())
              paste0("(", paste(head(args, 6L), collapse = ", "), if (length(args) > 6L) ", ..." else "", ")")
            } else ""
            paste(encode_hex(sym), encode_hex(kind_str), encode_hex(cls_str), encode_hex(sig_str), sep = "\t")
          }, character(1L))
          cat(paste(entries, collapse = "\n"))
        }
      } else if (meta_kind == "GETWD") {
        cat(encode_hex(enc2utf8(getwd())))
      }
    } else if (mode == "INSPECT_RDATA") {
      file_path <- payload
      if (!file.exists(file_path)) stop(sprintf("RData file does not exist: %s", file_path))
      temp_env <- new.env(parent = emptyenv())
      loaded <- tryCatch(
        load(file_path, envir = temp_env),
        error = function(e) stop(sprintf("Failed to load RData file: %s", conditionMessage(e)))
      )
      obj_names <- ls(envir = temp_env, all.names = TRUE)
      global_names <- ls(envir = .GlobalEnv, all.names = TRUE)
      collisions <- intersect(obj_names, global_names)
      encoded_names <- if (length(obj_names) > 0L) vapply(obj_names, encode_hex, character(1L)) else character()
      encoded_collisions <- if (length(collisions) > 0L) vapply(collisions, encode_hex, character(1L)) else character()
      cat(paste(length(obj_names), length(collisions), paste(encoded_names, collapse = ","), paste(encoded_collisions, collapse = ","), sep = "\t"))
    } else if (mode == "APPLY_RDATA") {
      parts <- strsplit(payload, "\t", fixed = TRUE)[[1L]]
      if (length(parts) != 2L) stop("Invalid APPLY_RDATA request")
      policy <- parts[[1L]]
      file_path <- parts[[2L]]
      if (!file.exists(file_path)) stop(sprintf("RData file does not exist: %s", file_path))
      temp_env <- new.env(parent = emptyenv())
      loaded <- tryCatch(
        load(file_path, envir = temp_env),
        error = function(e) stop(sprintf("Failed to load RData file: %s", conditionMessage(e)))
      )
      obj_names <- ls(envir = temp_env, all.names = TRUE)
      global_names <- ls(envir = .GlobalEnv, all.names = TRUE)
      to_import <- if (policy == "SKIP") {
        setdiff(obj_names, global_names)
      } else {
        obj_names
      }
      skipped_count <- length(obj_names) - length(to_import)
      for (name in to_import) {
        if (bindingIsActive(name, temp_env)) {
          active_fn <- activeBindingFunction(name, temp_env)
          makeActiveBinding(name, active_fn, .GlobalEnv)
        } else {
          assign(name, get(name, envir = temp_env, inherits = FALSE), envir = .GlobalEnv)
        }
      }
      cat(paste(length(to_import), skipped_count, sep = "\t"))
    } else if (mode == "SAVE_RDATA") {
      file_path <- gsub("\\", "/", payload, fixed = TRUE)
      dir_path <- dirname(file_path)
      if (!dir.exists(dir_path)) dir.create(dir_path, recursive = TRUE)
      tryCatch({
        save(list = ls(envir = .GlobalEnv, all.names = TRUE), file = file_path, envir = .GlobalEnv)
        if (!file.exists(file_path)) stop("R did not create the workspace file")
        obj_count <- length(ls(envir = .GlobalEnv, all.names = TRUE))
        file_sz <- file.size(file_path)
        if (is.na(file_sz) || length(file_sz) == 0L) file_sz <- file.info(file_path)$size
        cat(paste(obj_count, file_sz, sep = "\t"))
      }, error = function(e) {
        if (file.exists(file_path)) unlink(file_path)
        stop(sprintf("R save() error: %s", conditionMessage(e)))
      })
    } else {
      stop("Unknown R bridge request")
    }
  }), error = function(condition) {
    captured_error <<- conditionMessage(condition)
    character()
  })
  output_text <- paste(captured_output, collapse = "\n")
  warning_text <- paste(c(captured_warnings, captured_messages), collapse = "\n")
  status <- if (nzchar(captured_error)) "ERROR" else "OK"
  object_names <- ls(envir = .GlobalEnv, all.names = TRUE)
  object_metadata <- paste(vapply(object_names, function(name) {
    fields <- tryCatch({
      if (bindingIsActive(name, .GlobalEnv)) {
        c(name, "active_binding", "active_binding", "", "0", "", "Values", "Active binding (unevaluated)", "")
      } else {
        object <- get(name, envir = .GlobalEnv, inherits = FALSE)
        classes <- paste(class(object), collapse = ",")
        dimensions <- dim(object)
        dimensions_text <- if (is.null(dimensions)) "" else paste(dimensions, collapse = "x")
        preview <- if (is.atomic(object) && length(object) == 1L && is.null(dimensions)) paste(capture.output(dput(object)), collapse = "") else ""
        if (nchar(preview, type = "bytes") > 120L) preview <- paste0(substr(preview, 1L, 117L), "...")
        details <- describe_object(name, object, class(object), dimensions)
        c(name, classes, typeof(object), dimensions_text, as.character(length(object)), preview, details$category, details$description, plot_kind(object))
      }
    }, error = function(condition) c(name, "unknown", "unknown", "", "0", "", "Values", "Metadata unavailable", ""))
    paste(vapply(fields, encode_hex, character(1L)), collapse = ":")
  }, character(1L)), collapse = "\n")
  response <- paste("VCSTUDIO", request_id, status, encode_hex(output_text),
                    encode_hex(warning_text), encode_hex(captured_error), encode_hex(object_metadata),
                    encode_hex(enc2utf8(getwd())), sep = "\t")
  response_tmp <- paste0(response_path, ".tmp")
  writeLines(response, response_tmp, useBytes = TRUE)
  file.rename(response_tmp, response_path)
}
})
"#;

pub(crate) struct RSession {
    process: Option<RProcess>,
    next_request_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RObject {
    pub(crate) variable: String,
    pub(crate) classes: Vec<String>,
    pub(crate) data_type: String,
    pub(crate) dimensions: String,
    pub(crate) length: usize,
    pub(crate) preview: String,
    pub(crate) category: String,
    pub(crate) description: String,
    pub(crate) plot_kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RPackage {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) loaded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImportCollisionPolicy {
    Overwrite,
    Skip,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RDataInspection {
    pub(crate) file_path: String,
    pub(crate) total_objects: usize,
    pub(crate) object_names: Vec<String>,
    pub(crate) collisions: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct Execution {
    pub(crate) output: String,
    pub(crate) objects: Vec<RObject>,
    pub(crate) error: Option<String>,
    pub(crate) working_directory: Option<std::path::PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ParseState {
    Complete,
    Incomplete,
    Invalid(String),
}

struct RProcess {
    child: Child,
    request_path: std::path::PathBuf,
    response_path: std::path::PathBuf,
    console_path: std::path::PathBuf,
    stderr_path: std::path::PathBuf,
}

impl Default for RSession {
    fn default() -> Self {
        Self { process: None, next_request_id: 1 }
    }
}

/// Prevent a console window for the app-owned background R process on Windows.
#[cfg(windows)]
pub(crate) fn configure_background_r_command(command: &mut Command) {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
    // Windows R does not accept the agent's Unix C.UTF-8 locale. Let R use the user's
    // Windows locale so non-ASCII object names survive ls() and metadata transfer.
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if std::env::var(key).is_ok_and(|value| value.eq_ignore_ascii_case("C.UTF-8") || value.eq_ignore_ascii_case("C.UTF8")) {
            command.env_remove(key);
        }
    }
}

impl RSession {
    pub(crate) fn start(&mut self) -> Result<String, String> {
        if let Some(process) = self.process.as_mut() {
            match process.child.try_wait() {
                Ok(None) => return Ok("R session is already running".into()),
                Ok(Some(_status)) => {
                    self.process = None;
                }
                Err(error) => {
                    self.process = None;
                    return Err(format!("Could not check the R process: {error}"));
                }
            }
        }
        let executable = find_rscript()?;
        let session_id = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
        let session_dir = std::env::temp_dir().join(format!("vectorcraft-r-{}-{session_id}", std::process::id()));
        std::fs::create_dir(&session_dir).map_err(|error| format!("Could not create R session directory: {error}"))?;
        let request_path = session_dir.join("request.txt");
        let response_path = session_dir.join("response.txt");
        let bridge_path = session_dir.join("bridge.R");
        let stderr_path = session_dir.join("stderr.log");
        std::fs::write(&bridge_path, BRIDGE).map_err(|error| format!("Could not write the R session bridge: {error}"))?;
        let stderr = std::fs::File::create(&stderr_path).map_err(|error| format!("Could not create R diagnostic log: {error}"))?;
        let mut command = Command::new(&executable);
        command
            .arg("--vanilla")
            .arg("--slave")
            .arg(&bridge_path)
            .arg(&request_path)
            .arg(&response_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr));
        #[cfg(windows)]
        configure_background_r_command(&mut command);
        let child = command.spawn().map_err(|error| {
            let _ = std::fs::remove_dir_all(&session_dir);
            format!("Could not start {}: {error}", executable.display())
        })?;
        let console_path = session_dir.join("console.log");
        self.process = Some(RProcess { child, request_path, response_path, console_path, stderr_path });
        Ok(format!("R session started ({})", executable.display()))
    }

    pub(crate) fn stop(&mut self) -> Result<String, String> {
        let Some(mut process) = self.process.take() else {
            return Ok("R session is stopped".into());
        };
        process.request_stop();
        if process.child.try_wait().ok().flatten().is_none() {
            process.child.kill().map_err(|error| format!("Could not stop R: {error}"))?;
        }
        process.child.wait().map_err(|error| format!("Could not reap R process: {error}"))?;
        Ok("R session stopped".into())
    }

    pub(crate) fn restart(&mut self) -> Result<String, String> {
        let _ = self.stop();
        self.start().map(|message| message.replace("R session started", "R session restarted"))
    }

    pub(crate) fn execute(&mut self, code: &str) -> Result<Execution, String> {
        if code.len() > 16 * 1024 * 1024 {
            return Err("R request is larger than the 16 MiB session limit".into());
        }
        self.request("EXEC", code)
    }

    pub(crate) fn execute_streaming(&mut self, code: &str, on_output: &mut dyn FnMut(String)) -> Result<Execution, String> {
        if code.len() > 16 * 1024 * 1024 {
            return Err("R request is larger than the 16 MiB session limit".into());
        }
        self.request_inner("EXEC_STREAM", code, Some(on_output))
    }

    pub(crate) fn parse_input(&mut self, source: &str) -> Result<ParseState, String> {
        if source.len() > 16 * 1024 * 1024 {
            return Err("R input is larger than the 16 MiB session limit".into());
        }
        let result = self.request("PARSE", source)?;
        if let Some(error) = result.error {
            return Ok(ParseState::Invalid(error));
        }
        match result.output.trim() {
            "COMPLETE" => Ok(ParseState::Complete),
            "INCOMPLETE" => Ok(ParseState::Incomplete),
            _ => Err("R returned an invalid parse status".into()),
        }
    }

    pub(crate) fn remove_object(&mut self, name: &str) -> Result<Execution, String> {
        self.request("REMOVE", name)
    }

    pub(crate) fn clear_environment(&mut self) -> Result<Execution, String> {
        self.request("CLEAR", "")
    }

    pub(crate) fn help_text(&mut self, topic: &str) -> Result<String, String> {
        let topic = serde_json::to_string(topic).map_err(|error| format!("Could not encode R help topic: {error}"))?;
        let code = format!(
            "local({{ topic <- {topic}; file <- utils::help(topic); if (!length(file)) stop('No installed documentation for this topic'); text <- tools::Rd2txt(utils:::.getHelpFile(file)); cat(substr(text, 1L, 40000L)) }})"
        );
        let result = self.execute(&code)?;
        result.error.map_or(Ok(result.output), Err)
    }

    pub(crate) fn packages(&mut self) -> Result<Vec<RPackage>, String> {
        let result = self.execute(
            "local({ packages <- utils::installed.packages(); loaded <- loadedNamespaces(); cat(paste(sprintf('%s\\t%s\\t%s', packages[, 'Package'], packages[, 'Version'], ifelse(packages[, 'Package'] %in% loaded, 'loaded', 'installed')), collapse='\\n')) })",
        )?;
        if let Some(error) = result.error {
            return Err(error);
        }
        Ok(result
            .output
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let name = fields.next()?.to_owned();
                let version = fields.next()?.to_owned();
                let loaded = fields.next()? == "loaded";
                Some(RPackage { name, version, loaded })
            })
            .collect::<Vec<_>>())
    }

    #[allow(dead_code)]
    pub(crate) fn complete_expression_at(&mut self, script: &str, cursor_line: usize) -> Result<(String, usize, usize, Option<usize>), String> {
        if script.len() > 16 * 1024 * 1024 {
            return Err("R script is larger than the 16 MiB session limit".into());
        }
        let payload = format!("{}\n{script}", cursor_line.saturating_add(1));
        let result = self.request("EXPR", &payload)?;
        if let Some(error) = result.error {
            return Err(error);
        }
        let output = result.output.trim_end_matches('\n');
        let (span, expression) = output.split_once('\n').ok_or_else(|| "R returned an invalid expression span".to_owned())?;
        let mut fields = span.split('\t');
        let start_line = fields.next().ok_or_else(|| "R returned an invalid expression span".to_owned())?;
        let end_line = fields.next().ok_or_else(|| "R returned an invalid expression span".to_owned())?;
        let next_line = fields.next().ok_or_else(|| "R returned an invalid expression span".to_owned())?;
        let start_line = start_line.parse::<usize>().map_err(|_| "R returned an invalid expression start line".to_owned())?;
        let end_line = end_line.parse::<usize>().map_err(|_| "R returned an invalid expression end line".to_owned())?;
        if start_line == 0 || end_line < start_line {
            return Err("R returned an out-of-range expression span".into());
        }
        let next_line = next_line.parse::<usize>().map_err(|_| "R returned an invalid next expression line".to_owned())?;
        Ok((expression.to_owned(), start_line - 1, end_line - 1, (next_line > 0).then_some(next_line.saturating_sub(1))))
    }

    pub(crate) fn assignment_definitions(&mut self, script: &str) -> Result<Vec<(String, usize)>, String> {
        let result = self.request("DEFS", script)?;
        if let Some(error) = result.error {
            return Err(error);
        }
        result
            .output
            .lines()
            .map(|line| {
                let (encoded, line) = line.split_once('\t').ok_or_else(|| "R returned an invalid definition".to_owned())?;
                let name = decode_hex(encoded).ok_or_else(|| "R returned an invalid definition name".to_owned())?;
                let line = line.parse::<usize>().map_err(|_| "R returned an invalid definition line".to_owned())?;
                Ok((name, line.saturating_sub(1)))
            })
            .collect()
    }

    pub(crate) fn data_page(&mut self, name: &str, start: usize, page_size: usize) -> Result<DataPage, String> {
        let payload = format!("{name}\t{start}\t{}", page_size.clamp(1, 200));
        let result = self.request("DATA", &payload)?;
        if let Some(error) = result.error {
            return Err(error);
        }
        parse_data_page(&result.output)
    }

    pub(crate) fn completion_metadata(&mut self, payload: &str) -> Result<String, String> {
        let result = self.request("COMPLETE_META", payload)?;
        if let Some(error) = result.error {
            return Err(error);
        }
        Ok(result.output)
    }

    #[allow(dead_code)]
    pub(crate) fn get_working_directory(&mut self) -> Result<std::path::PathBuf, String> {
        let raw = self.completion_metadata("GETWD")?;
        let decoded = decode_hex(&raw).ok_or_else(|| "Failed to decode getwd response".to_string())?;
        Ok(std::path::PathBuf::from(decoded))
    }

    pub(crate) fn inspect_rdata(&mut self, path: &str) -> Result<RDataInspection, String> {
        let result = self.request("INSPECT_RDATA", path)?;
        if let Some(error) = result.error {
            return Err(error);
        }
        parse_rdata_inspection(path, &result.output)
    }

    pub(crate) fn apply_rdata_import(&mut self, path: &str, policy: ImportCollisionPolicy) -> Result<(Execution, usize, usize), String> {
        let policy_str = match policy {
            ImportCollisionPolicy::Overwrite => "OVERWRITE",
            ImportCollisionPolicy::Skip => "SKIP",
        };
        let payload = format!("{policy_str}\t{path}");
        let result = self.request("APPLY_RDATA", &payload)?;
        if let Some(error) = &result.error {
            return Err(error.clone());
        }
        let (imported, skipped) = parse_apply_rdata_output(&result.output)?;
        Ok((result, imported, skipped))
    }

    pub(crate) fn save_rdata(&mut self, path: &str) -> Result<(usize, u64), String> {
        let result = self.request("SAVE_RDATA", path)?;
        if let Some(error) = result.error {
            return Err(error);
        }
        parse_save_rdata_output(&result.output)
    }

    #[cfg(test)]
    pub(crate) fn render_plot_svg(&mut self, variable: &str) -> Result<(String, Vec<RObject>), String> {
        self.render_plot_svg_at_size(variable, 177.8, 127.0)
    }

    #[cfg(test)]
    pub(crate) fn render_plot_svg_at_size(&mut self, variable: &str, width_mm: f64, height_mm: f64) -> Result<(String, Vec<RObject>), String> {
        self.render_plot_svg_with_style(variable, width_mm, height_mm, false)
    }

    #[cfg(test)]
    pub(crate) fn render_plot_svg_with_style(
        &mut self,
        variable: &str,
        width_mm: f64,
        height_mm: f64,
        small_figure: bool,
    ) -> Result<(String, Vec<RObject>), String> {
        self.render_plot_svg_with_graph_style(variable, width_mm, height_mm, small_figure, &crate::scientific::graph_style::GraphStyle::default())
    }

    pub(crate) fn render_plot_svg_with_graph_style(
        &mut self,
        variable: &str,
        width_mm: f64,
        height_mm: f64,
        small_figure: bool,
        graph_style: &crate::scientific::graph_style::GraphStyle,
    ) -> Result<(String, Vec<RObject>), String> {
        if !width_mm.is_finite() || !height_mm.is_finite() || width_mm <= 0.0 || height_mm <= 0.0 || width_mm > 1000.0 || height_mm > 1000.0 {
            return Err("Plot dimensions must be finite and between 0 and 1000 mm".into());
        }
        self.start()?;
        let process = self.process.as_ref().ok_or_else(|| "R session stopped after rendering".to_owned())?;
        let path = process.request_path.with_file_name("plot.svg");
        let payload = format!(
            "{}\t{width_mm:.9}\t{height_mm:.9}\t{}\t{}",
            encode_hex(variable.as_bytes()),
            if small_figure { "small" } else { "original" },
            graph_style.wire()?
        );
        let result = self.request("RENDER", &payload).and_then(|result| {
            if let Some(error) = result.error {
                return Err(error);
            }
            let metadata = std::fs::metadata(&path).map_err(|error| format!("R did not produce SVG output: {error}"))?;
            if metadata.len() > 16 * 1024 * 1024 {
                return Err("Rendered SVG exceeds the 16 MiB limit".into());
            }
            let svg = std::fs::read_to_string(&path).map_err(|error| format!("Could not read rendered SVG: {error}"))?;
            Ok((svg, result.objects))
        });
        let _ = std::fs::remove_file(&path);
        result
    }

    fn request(&mut self, mode: &str, payload: &str) -> Result<Execution, String> {
        self.request_inner(mode, payload, None)
    }

    fn request_inner(&mut self, mode: &str, payload: &str, mut on_output: Option<&mut dyn FnMut(String)>) -> Result<Execution, String> {
        self.start()?;
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        let encoded_payload = if payload.is_empty() { "-".to_owned() } else { encode_hex(payload.as_bytes()) };
        if matches!(mode, "EXEC" | "EXEC_STREAM") && source_diagnostics_enabled() {
            let recovered = if encoded_payload == "-" { Some(Vec::new()) } else { decode_hex(&encoded_payload).map(|text| text.into_bytes()) };
            let diff = recovered.as_deref().and_then(|bytes| first_differing_byte(payload.as_bytes(), bytes));
            eprintln!(
                "[r-source-diagnostic] stage=ipc-send encoding=utf-8 source_bytes={} source_fnv1a64={:016x} payload_bytes={} payload_fnv1a64={:016x} first_diff_offset={}",
                payload.len(),
                source_diagnostic_hash(payload.as_bytes()),
                recovered.as_ref().map_or(0, Vec::len),
                recovered.as_deref().map_or(0, source_diagnostic_hash),
                diff.map_or_else(|| "none".to_owned(), |offset| offset.to_string()),
            );
        }
        let request = format!("{request_id}\t{mode}\t{encoded_payload}");
        let Some(process) = self.process.as_mut() else {
            return Err("R process was not available after startup".into());
        };
        let execution_mode = matches!(mode, "EXEC" | "EXEC_STREAM");
        if execution_mode {
            let _ = std::fs::remove_file(&process.console_path);
        }
        let temporary_request = process.request_path.with_extension("pending");
        std::fs::write(&temporary_request, request).map_err(|error| format!("Could not stage R request: {error}"))?;
        std::fs::rename(&temporary_request, &process.request_path).map_err(|error| format!("Could not send request to R: {error}"))?;
        let mut console_offset = 0_u64;
        let mut console_buffer = Vec::new();
        let mut execution_output = String::new();
        let mut deliver_output = |chunk: String| {
            if let Some(callback) = on_output.as_deref_mut() {
                callback(chunk);
            } else {
                execution_output.push_str(&chunk);
            }
        };
        loop {
            if execution_mode {
                drain_console_log(&process.console_path, &mut console_offset, &mut console_buffer, false, &mut deliver_output)?;
            }
            if process.response_path.is_file() {
                let line = std::fs::read_to_string(&process.response_path).map_err(|error| format!("Could not read the R response: {error}"))?;
                let _ = std::fs::remove_file(&process.response_path);
                if let Some(response) = parse_response(&line, request_id) {
                    if execution_mode {
                        drain_console_log(&process.console_path, &mut console_offset, &mut console_buffer, true, &mut deliver_output)?;
                        let _ = std::fs::remove_file(&process.console_path);
                    }
                    let mut text = String::new();
                    if execution_mode {
                        text.push_str(&execution_output);
                    } else if !response.output.is_empty() {
                        text.push_str(&response.output);
                        text.push('\n');
                    }
                    if !execution_mode && !response.warnings.is_empty() {
                        text.push_str("Warning: ");
                        text.push_str(&response.warnings.replace('\n', "\nWarning: "));
                        text.push('\n');
                    }
                    return Ok(Execution {
                        output: text,
                        objects: response.objects,
                        error: (!response.error.is_empty()).then_some(response.error),
                        working_directory: response.working_directory,
                    });
                }
            }
            if let Some(status) = process.child.try_wait().map_err(|error| format!("Could not check R: {error}"))? {
                let diagnostics = std::fs::read_to_string(&process.stderr_path).unwrap_or_default();
                self.process = None;
                return Err(format!(
                    "R session exited before returning a result ({status}){}",
                    if diagnostics.trim().is_empty() { String::new() } else { format!(": {}", diagnostics.trim()) }
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

fn drain_console_log(
    path: &std::path::Path,
    offset: &mut u64,
    pending: &mut Vec<u8>,
    flush_partial: bool,
    on_output: &mut dyn FnMut(String),
) -> Result<(), String> {
    use std::io::{Read as _, Seek as _, SeekFrom};

    let Ok(metadata) = std::fs::metadata(path) else { return Ok(()) };
    if metadata.len() < *offset {
        *offset = 0;
        pending.clear();
    }
    if metadata.len() > *offset {
        let mut file = std::fs::File::open(path).map_err(|error| format!("Could not read R console output: {error}"))?;
        file.seek(SeekFrom::Start(*offset)).map_err(|error| format!("Could not seek R console output: {error}"))?;
        let mut new_bytes = Vec::new();
        file.read_to_end(&mut new_bytes).map_err(|error| format!("Could not read R console output: {error}"))?;
        *offset = offset.saturating_add(new_bytes.len() as u64);
        pending.extend_from_slice(&new_bytes);
    }
    let candidate_len = if flush_partial || pending.len() >= 64 * 1024 {
        pending.len()
    } else {
        pending.iter().rposition(|byte| *byte == b'\n').map_or(0, |index| index + 1)
    };
    let mut emit_len = if flush_partial {
        candidate_len
    } else {
        match std::str::from_utf8(pending.get(..candidate_len).unwrap_or_default()) {
            Ok(_) => candidate_len,
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => candidate_len,
        }
    };
    if !flush_partial && pending.get(..emit_len).is_some_and(|bytes| bytes.ends_with(b"\r")) {
        emit_len = emit_len.saturating_sub(1);
    }
    if emit_len > 0 {
        let bytes: Vec<_> = pending.drain(..emit_len).collect();
        on_output(String::from_utf8_lossy(&bytes).replace("\r\n", "\n"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DataPage {
    pub(crate) total_rows: usize,
    pub(crate) columns: Vec<(String, String)>,
    pub(crate) rows: Vec<Vec<String>>,
}

fn parse_data_page(output: &str) -> Result<DataPage, String> {
    let mut lines = output.lines();
    let header = lines.next().ok_or_else(|| "R returned an empty data page".to_owned())?;
    let mut parts = header.splitn(3, '|');
    if parts.next() != Some("DATA") {
        return Err("R returned an invalid data page".into());
    }
    let total_rows = parts.next().and_then(|value| value.parse().ok()).ok_or_else(|| "R returned an invalid row count".to_owned())?;
    let columns = parts
        .next()
        .unwrap_or_default()
        .split('\t')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (name, data_type) = entry.split_once(':').ok_or_else(|| "R returned invalid column metadata".to_owned())?;
            Ok((
                decode_hex(name).ok_or_else(|| "R returned invalid column name".to_owned())?,
                decode_hex(data_type).ok_or_else(|| "R returned invalid column type".to_owned())?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let rows = lines
        .map(|line| {
            line.split('\t').map(|cell| decode_hex(cell).ok_or_else(|| "R returned invalid cell data".to_owned())).collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    if rows.iter().any(|row| row.len() != columns.len()) {
        return Err("R returned rows with inconsistent column counts".into());
    }
    Ok(DataPage { total_rows, columns, rows })
}

impl Drop for RProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            self.request_stop();
            if self.child.try_wait().ok().flatten().is_none() {
                // The session is app-owned; closing the app must not leave R running behind it.
                let _ = self.child.kill();
            }
            let _ = self.child.wait();
        }
        if let Some(directory) = self.request_path.parent() {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}

impl RProcess {
    fn request_stop(&mut self) {
        let temporary_request = self.request_path.with_extension("pending");
        if std::fs::write(&temporary_request, "0\tSTOP\t0").is_ok() {
            let _ = std::fs::rename(&temporary_request, &self.request_path);
        }
        for _ in 0..50 {
            if self.child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

struct Response {
    output: String,
    warnings: String,
    error: String,
    objects: Vec<RObject>,
    working_directory: Option<std::path::PathBuf>,
}

fn parse_response(line: &str, request_id: u64) -> Option<Response> {
    let fields: Vec<_> = line.trim_end_matches(['\r', '\n']).split('\t').collect();
    if fields.len() < 7 || fields.first().copied() != Some("VCSTUDIO") || fields.get(1).copied() != Some(&request_id.to_string()) {
        return None;
    }
    let metadata = decode_hex(fields.get(6).copied()?)?;
    let objects = metadata
        .lines()
        .filter_map(|entry| {
            let values: Vec<_> = entry.split(':').collect();
            let get = |index: usize| values.get(index).and_then(|value| decode_hex(value));
            Some(RObject {
                variable: get(0)?,
                classes: get(1)?.split(',').map(str::to_owned).collect(),
                data_type: get(2)?,
                dimensions: get(3)?,
                length: get(4)?.parse().ok()?,
                preview: get(5)?,
                category: get(6)?,
                description: get(7)?,
                plot_kind: get(8)?,
            })
        })
        .collect();
    let working_directory = fields.get(7).copied().and_then(decode_hex).map(std::path::PathBuf::from);
    Some(Response {
        output: decode_hex(fields.get(3).copied()?)?,
        warnings: decode_hex(fields.get(4).copied()?)?,
        error: if fields.get(2).copied()? == "ERROR" { decode_hex(fields.get(5).copied()?)? } else { String::new() },
        objects,
        working_directory,
    })
}

fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn source_diagnostics_enabled() -> bool {
    std::env::var_os("VECTORCRAFT_R_SOURCE_DIAGNOSTICS").as_deref() == Some(std::ffi::OsStr::new("1"))
}

fn first_differing_byte(left: &[u8], right: &[u8]) -> Option<usize> {
    left.iter().zip(right).position(|(left, right)| left != right).or_else(|| (left.len() != right.len()).then_some(left.len().min(right.len())))
}

fn source_diagnostic_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3))
}

pub(crate) fn decode_hex(value: &str) -> Option<String> {
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    let mut bytes = Vec::with_capacity(pairs.len());
    for pair in pairs {
        let digits = std::str::from_utf8(pair).ok()?;
        bytes.push(u8::from_str_radix(digits, 16).ok()?);
    }
    String::from_utf8(bytes).ok()
}

pub(crate) fn parse_rdata_inspection(path: &str, output: &str) -> Result<RDataInspection, String> {
    let mut parts = output.trim().split('\t');
    let total_str = parts.next().ok_or_else(|| "Invalid inspection output: missing total".to_string())?;
    let collision_str = parts.next().ok_or_else(|| "Invalid inspection output: missing collisions count".to_string())?;
    let names_str = parts.next().unwrap_or("");
    let collisions_str = parts.next().unwrap_or("");

    let total_objects = total_str.parse::<usize>().map_err(|e| format!("Invalid total objects: {e}"))?;
    let _collision_count = collision_str.parse::<usize>().map_err(|e| format!("Invalid collision count: {e}"))?;

    let object_names = if names_str.is_empty() {
        Vec::new()
    } else {
        names_str
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|hex| decode_hex(hex).ok_or_else(|| "Failed to decode object name".to_string()))
            .collect::<Result<Vec<_>, _>>()?
    };

    let collisions = if collisions_str.is_empty() {
        Vec::new()
    } else {
        collisions_str
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|hex| decode_hex(hex).ok_or_else(|| "Failed to decode collision name".to_string()))
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(RDataInspection { file_path: path.to_string(), total_objects, object_names, collisions })
}

pub(crate) fn parse_apply_rdata_output(output: &str) -> Result<(usize, usize), String> {
    let mut parts = output.trim().split('\t');
    let imported_str = parts.next().ok_or_else(|| "Invalid apply output: missing imported count".to_string())?;
    let skipped_str = parts.next().ok_or_else(|| "Invalid apply output: missing skipped count".to_string())?;
    let imported = imported_str.parse::<usize>().map_err(|e| format!("Invalid imported count: {e}"))?;
    let skipped = skipped_str.parse::<usize>().map_err(|e| format!("Invalid skipped count: {e}"))?;
    Ok((imported, skipped))
}

pub(crate) fn parse_save_rdata_output(output: &str) -> Result<(usize, u64), String> {
    let mut parts = output.trim().split('\t');
    let count_str = parts.next().ok_or_else(|| "Invalid save output: missing object count".to_string())?;
    let size_str = parts.next().ok_or_else(|| "Invalid save output: missing file size".to_string())?;
    let count = count_str.parse::<usize>().map_err(|e| format!("Invalid object count in SAVE_RDATA: {e}"))?;
    let size = size_str.parse::<u64>().map_err(|e| format!("Invalid file size in SAVE_RDATA: {e}"))?;
    Ok((count, size))
}

pub(crate) fn find_rscript() -> Result<std::path::PathBuf, String> {
    find_rscript_with(|name| std::env::var_os(name).map(std::path::PathBuf::from))
}

pub(crate) fn find_rscript_with(lookup_env: impl Fn(&str) -> Option<std::path::PathBuf>) -> Result<std::path::PathBuf, String> {
    let executable_name = if cfg!(windows) { "Rscript.exe" } else { "Rscript" };
    if let Some(home) = lookup_env("R_HOME") {
        for sub in ["bin", "bin/x64"] {
            let candidate = home.join(sub).join(executable_name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    if let Some(path) = lookup_env("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(executable_name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    #[cfg(windows)]
    {
        for env_var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "LOCALAPPDATA"] {
            if let Some(base) = lookup_env(env_var) {
                let r_root = if env_var == "LOCALAPPDATA" { base.join("Programs").join("R") } else { base.join("R") };
                if let Ok(entries) = std::fs::read_dir(r_root) {
                    let mut candidates: Vec<_> = entries
                        .filter_map(Result::ok)
                        .filter(|entry| entry.file_name().to_string_lossy().starts_with("R-"))
                        .map(|entry| entry.path())
                        .collect();
                    candidates.sort();
                    for version in candidates.into_iter().rev() {
                        for bin in [version.join("bin"), version.join("bin").join("x64")] {
                            let candidate = bin.join(executable_name);
                            if candidate.is_file() {
                                return Ok(candidate);
                            }
                        }
                    }
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        for dir in ["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin", "/Library/Frameworks/R.framework/Resources/bin"] {
            let candidate = std::path::PathBuf::from(dir).join(executable_name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err("Rscript was not found on PATH, under R_HOME, or in standard R install folders".into())
}

#[cfg(test)]
mod tests {
    use super::{RSession, decode_hex, encode_hex};
    use crate::scientific::graph_style::{FontFace, GraphStyle, LegendDirection, LegendPosition, TextDetails};

    #[test]
    fn protocol_hex_round_trips_unicode_and_empty_text() {
        for value in ["", "R code + newline\n", "\0non-ascii: λ"] {
            assert_eq!(decode_hex(&encode_hex(value.as_bytes())).as_deref(), Some(value));
        }
    }

    #[test]
    fn bridge_receives_the_exact_utf8_payload_after_hex_ipc_decoding() {
        if super::find_rscript().is_err() {
            return;
        }
        let source = "# 한글 주석\n값 <- \"quote: \\\" and slash: \\\\\"\nraw <- r\"(C:\\Users\\rugh)\"\n";
        let mut session = RSession::default();
        let result = session.request("VERIFY_PAYLOAD", source).unwrap();
        assert_eq!(result.error, None);
        assert_eq!(result.output.trim(), encode_hex(source.as_bytes()));
        assert!(session.stop().is_ok());
    }

    #[test]
    fn streaming_keeps_partial_utf8_sequences_until_the_next_file_read() {
        let path = std::env::temp_dir().join(format!("vectorcraft-console-utf8-{}.log", std::process::id()));
        std::fs::write(&path, [0xE2, 0x82]).unwrap();
        let mut offset = 0;
        let mut pending = Vec::new();
        let mut output = String::new();
        super::drain_console_log(&path, &mut offset, &mut pending, false, &mut |chunk| output.push_str(&chunk)).unwrap();
        assert!(output.is_empty());
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        use std::io::Write as _;
        file.write_all(&[0xAC, b'\n']).unwrap();
        super::drain_console_log(&path, &mut offset, &mut pending, false, &mut |chunk| output.push_str(&chunk)).unwrap();
        assert_eq!(output, "€\n");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn one_session_shares_objects_and_can_restart() {
        let mut session = RSession::default();
        if session.start().is_err() {
            return;
        }
        assert_eq!(session.execute("shared_value <- 40").map(|result| result.output).as_deref(), Ok(""));
        assert_eq!(session.execute("shared_value + 2").map(|result| result.output).as_deref(), Ok("[1] 42\n"));
        assert!(session.execute("warning('careful')").map(|result| result.output).unwrap_or_default().contains("Warning: careful"));
        assert!(
            session
                .execute("stop('bad expression')")
                .is_ok_and(|result| result.error.as_deref().is_some_and(|error| error.contains("bad expression")))
        );
        assert_eq!(session.execute("shared_value").map(|result| result.output).as_deref(), Ok("[1] 40\n"));
        assert!(session.restart().is_ok());
        assert!(session.execute("shared_value").is_ok_and(|result| result.error.is_some()));
        assert!(session.stop().is_ok());
    }

    #[test]
    fn interactive_parser_distinguishes_complete_incomplete_and_invalid_input() {
        use super::ParseState;
        if super::find_rscript().is_err() {
            return;
        }
        let mut session = RSession::default();
        assert_eq!(session.parse_input("value <- 1").unwrap(), ParseState::Complete);
        assert_eq!(session.parse_input("value <- function(x) {").unwrap(), ParseState::Incomplete);
        assert!(matches!(session.parse_input("value <- )").unwrap(), ParseState::Invalid(message) if message.contains("unexpected")));
        assert_eq!(session.parse_input("'unfinished").unwrap(), ParseState::Incomplete);
        assert!(session.execute("input_after_parse_error <- 42").unwrap().error.is_none());
        assert_eq!(session.execute("input_after_parse_error").unwrap().output.trim(), "[1] 42");
    }

    #[test]
    fn interactive_execution_streams_output_and_keeps_the_persistent_environment() {
        if super::find_rscript().is_err() {
            return;
        }
        let mut session = RSession::default();
        let mut streamed = String::new();
        let result = session
            .execute_streaming("stream_value <- 7\ncat('first line\\n')\nwarning('stream warning')\nstream_value", &mut |chunk| {
                streamed.push_str(&chunk)
            })
            .unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        assert!(streamed.contains("first line"), "{streamed:?}");
        assert!(streamed.contains("Warning: stream warning"), "{streamed:?}");
        assert!(streamed.contains("[1] 7"), "{streamed:?}");
        let mut large = String::new();
        session.execute_streaming("print(seq_len(10000))", &mut |chunk| large.push_str(&chunk)).unwrap();
        let normalized = large.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(normalized.contains("1 2 3"), "large output prefix: {:?}", large.get(..large.len().min(100)));
        assert!(normalized.contains("10000"), "large output suffix: {:?}", large.get(large.len().saturating_sub(100)..));
        assert!(session.execute("stopifnot(stream_value == 7)").unwrap().error.is_none());
        assert!(session.stop().is_ok());
    }

    #[test]
    fn buffered_and_streaming_execution_share_visibility_output_and_error_semantics() {
        if super::find_rscript().is_err() {
            return;
        }
        let mut session = RSession::default();
        let source = concat!(
            "buffer_stream_value <- 10\n",
            "1 + 1\n2 + 2\n",
            "invisible(42)\n",
            "buffer_stream_value\n",
            "message('ordered message')\n",
            "cat('cat output\\n')\n",
            "warning('ordered warning')\n",
            "print('printed output')\n"
        );
        let buffered = session.execute(source).unwrap();
        assert!(buffered.error.is_none(), "{:?}", buffered.error);
        let mut streamed_output = String::new();
        let streamed = session.execute_streaming(source, &mut |chunk| streamed_output.push_str(&chunk)).unwrap();
        assert!(streamed.error.is_none(), "{:?}", streamed.error);
        assert_eq!(streamed_output, buffered.output);
        let normalized_output = buffered.output.replace("\r\n", "\n");
        assert!(normalized_output.contains("[1] 2\n[1] 4\n"), "{:?}", normalized_output);
        assert!(!normalized_output.contains("42"), "invisible values must not print: {:?}", normalized_output);
        let message_at = normalized_output.find("Info: ordered message").unwrap();
        let cat_at = normalized_output.find("cat output").unwrap();
        let warning_at = normalized_output.find("Warning: ordered warning").unwrap();
        let print_at = normalized_output.find("printed output").unwrap();
        assert!(message_at < cat_at && cat_at < warning_at && warning_at < print_at, "{:?}", normalized_output);

        let runtime_failure = "preserved_after_error <- 17\nstop('intentional runtime failure')\nnot_reached <- TRUE";
        let buffered_error = session.execute(runtime_failure).unwrap();
        assert!(buffered_error.error.as_deref().is_some_and(|error| error.contains("intentional runtime failure")));
        let mut streamed_error_output = String::new();
        let streamed_error = session.execute_streaming(runtime_failure, &mut |chunk| streamed_error_output.push_str(&chunk)).unwrap();
        assert!(streamed_error.error.as_deref().is_some_and(|error| error.contains("intentional runtime failure")));
        assert_eq!(streamed_error_output, buffered_error.output);
        assert!(session.execute("stopifnot(preserved_after_error == 17, !exists('not_reached'))").unwrap().error.is_none());

        let parse_failure = session.execute("must_not_run <- 1\nvalue <- )").unwrap();
        assert!(parse_failure.error.is_some());
        assert!(session.execute("stopifnot(!exists('must_not_run'))").unwrap().error.is_none());
        assert!(session.execute("session_still_works <- 2").unwrap().error.is_none());
        assert!(session.stop().is_ok());
    }

    #[test]
    fn environment_inspection_data_pages_and_r_parser_share_the_live_session() {
        let mut session = RSession::default();
        if session.start().is_err() {
            return;
        }
        let setup = session.execute("df <- mtcars\nx <- 42\nmodel <- lm(mpg ~ wt, data = df)").unwrap();
        assert!(setup.error.is_none(), "{:?}", setup.error);
        let df = setup.objects.iter().find(|object| object.variable == "df").unwrap();
        assert!(df.classes.contains(&"data.frame".to_owned()));
        assert_eq!(df.dimensions, "32x11");
        assert_eq!(df.category, "Data");
        assert!(df.description.contains("32 obs. of 11 variables"), "{}", df.description);
        let scalar = setup.objects.iter().find(|object| object.variable == "x").unwrap();
        assert_eq!(scalar.data_type, "double");
        assert_eq!(scalar.preview, "42");
        assert_eq!(scalar.category, "Values");
        assert_eq!(scalar.description, "num 42");
        assert!(setup.objects.iter().any(|object| object.variable == "model" && object.classes.contains(&"lm".to_owned())));

        let classified = session
            .execute("mat <- matrix(1:12, nrow=3)\nplain_list <- list(a=1)\nlarge_vector <- seq_len(24708)\nassign('\\uD55C\\uAE00_\\uD568\\uC218', function(res_df) res_df, envir=.GlobalEnv)")
            .unwrap();
        assert!(classified.error.is_none(), "{:?}", classified.error);
        let matrix = classified.objects.iter().find(|object| object.variable == "mat").unwrap();
        assert_eq!(matrix.category, "Data");
        assert!(matrix.description.contains("3 x 4 matrix"));
        assert_eq!(classified.objects.iter().find(|object| object.variable == "plain_list").unwrap().category, "Values");
        let large = classified.objects.iter().find(|object| object.variable == "large_vector").unwrap();
        assert_eq!(large.category, "Values");
        assert!(large.description.contains("KB"), "{}", large.description);
        let function = classified
            .objects
            .iter()
            .find(|object| object.variable == "한글_함수")
            .unwrap_or_else(|| panic!("R names: {:?}", classified.objects.iter().map(|object| &object.variable).collect::<Vec<_>>()));
        assert_eq!(function.category, "Functions");
        assert_eq!(function.description, "function(res_df)");

        let source = "# comment\nplot_A <- ggplot(df, aes(wt, mpg)) +\n  geom_point() +\n  theme_classic()\nnext_value <- 1";
        let (expression, start_line, end_line, next_line) = session.complete_expression_at(source, 2).unwrap();
        assert!(expression.contains("plot_A <- ggplot"), "{expression}");
        assert!(expression.contains("theme_classic()"), "{expression}");
        assert!(!expression.contains("next_value"));
        assert_eq!((start_line, end_line), (1, 3));
        assert_eq!(next_line, Some(4));
        let (first_line, first_start, first_end, next_line) = session.complete_expression_at("library(Seurat)\n", 0).unwrap();
        assert_eq!(first_line, "library(Seurat)");
        assert_eq!((first_start, first_end), (0, 0));
        assert_eq!(next_line, None);

        let navigation = "text <- \"a # inside string\"\n\n# comment\nplot_A <- ggplot(df, aes(wt, mpg)) +\n  geom_point()\n\n# another comment\nplot_B <- ggplot(df, aes(wt, mpg)) +\n  geom_line()\n";
        let (_, _, _, next_line) = session.complete_expression_at(navigation, 0).unwrap();
        assert_eq!(next_line, Some(3));
        let (_, start, end, next_line) = session.complete_expression_at(navigation, 4).unwrap();
        assert_eq!((start, end, next_line), (3, 4, Some(7)));
        let definitions = session.assignment_definitions("# plot_B <- 9\nmessage <- 'plot_B <- 1'\nplot_B_backup <- 3\nplot_B <- ggplot(df, aes(wt, mpg)) +\n geom_point()\n7 -> `reverse name`\n").unwrap();
        assert_eq!(definitions.iter().filter(|(name, _)| name == "plot_B").count(), 1);
        assert!(definitions.contains(&("plot_B".into(), 3)));
        assert!(definitions.contains(&("reverse name".into(), 5)));

        let page = session.data_page("df", 10, 5).unwrap();
        assert_eq!(page.total_rows, 32);
        assert_eq!(page.columns.first().map(|column| column.0.as_str()), Some("mpg"));
        assert_eq!(page.rows.len(), 5);
        assert_eq!(page.rows.first().and_then(|row| row.first()).map(String::as_str), Some("17.8"));

        let removed = session.remove_object("x").unwrap();
        assert!(!removed.objects.iter().any(|object| object.variable == "x"));
        let cleared = session.clear_environment().unwrap();
        assert!(cleared.objects.is_empty());
        assert!(session.stop().is_ok());
    }

    #[test]
    fn ggplot_objects_are_discovered_and_rendered_as_vector_svg() {
        let mut session = RSession::default();
        let Ok(result) = session.execute("if (!requireNamespace('ggplot2', quietly=TRUE)) stop('missing ggplot2')") else { return };
        if result.error.is_some() {
            return;
        }
        let result = session
            .execute("phase3_plot <- ggplot2::ggplot(data.frame(x=1:3, y=1:3), ggplot2::aes(x, y)) + ggplot2::geom_point()")
            .unwrap_or_else(|error| panic!("R execution failed: {error}"));
        assert!(result.error.is_none(), "{:?}", result.error);
        assert!(result.objects.iter().any(|plot| plot.variable == "phase3_plot" && plot.classes.contains(&"ggplot".to_owned())));
        let (svg, plots) = session.render_plot_svg("phase3_plot").unwrap_or_else(|error| panic!("SVG render failed: {error}"));
        assert!(svg.contains("<svg"));
        assert!(plots.iter().any(|plot| plot.variable == "phase3_plot"));
        let mut document = vectorcraft_engine::Session::new();
        document.execute("file.new", &serde_json::json!({"width": 400, "height": 300})).unwrap();
        let imported = document.execute("clipboard.importSvg", &serde_json::json!({"svg": svg})).unwrap();
        assert!(imported["count"].as_u64().is_some_and(|count| count > 0));
        let pasted = document.execute("edit.pasteInPlace", &serde_json::json!({})).unwrap();
        assert!(pasted["ids"].as_array().is_some_and(|ids| !ids.is_empty()));
        assert!(session.stop().is_ok());
    }

    #[test]
    fn small_figure_ggplot_render_uses_temporary_smaller_text_without_changing_the_r_object() {
        let mut session = RSession::default();
        let available = session
            .execute("cat(if (requireNamespace('ggplot2', quietly=TRUE) && requireNamespace('svglite', quietly=TRUE)) 'YES' else 'NO')")
            .unwrap();
        if available.output.trim() != "YES" {
            return;
        }
        let created = session.execute("small_plot <- ggplot2::ggplot(mtcars, ggplot2::aes(wt, mpg)) + ggplot2::geom_point() + ggplot2::theme_bw() + ggplot2::labs(title='Figure title', x='Weight', y='MPG')").unwrap();
        assert!(created.error.is_none(), "{:?}", created.error);
        let before = session.execute("cat(small_plot$theme$text$size)").unwrap().output;
        let regular = session.render_plot_svg_with_style("small_plot", 70.0, 50.0, false).unwrap().0;
        let compact = session.render_plot_svg_with_style("small_plot", 70.0, 50.0, true).unwrap().0;
        let after = session.execute("cat(small_plot$theme$text$size)").unwrap().output;
        assert_eq!(before, after);
        assert!(compact.contains("<text") && compact.contains("<svg"));
        let sizes = |svg: &str| -> Vec<f64> {
            svg.split("font-size:")
                .skip(1)
                .filter_map(|part| part.trim_start().chars().take_while(|ch| ch.is_ascii_digit() || *ch == '.').collect::<String>().parse().ok())
                .collect()
        };
        let regular_sizes = sizes(&regular);
        let compact_sizes = sizes(&compact);
        assert!(!regular_sizes.is_empty() && regular_sizes.len() == compact_sizes.len());
        assert!(compact_sizes.iter().sum::<f64>() < regular_sizes.iter().sum::<f64>(), "regular={regular_sizes:?} compact={compact_sizes:?}");
        assert!(session.stop().is_ok());
    }

    #[test]
    fn graph_styles_render_distinct_svg_without_mutating_the_named_ggplot() {
        let mut session = RSession::default();
        let Ok(available) =
            session.execute("cat(if (requireNamespace('ggplot2', quietly=TRUE) && requireNamespace('svglite', quietly=TRUE)) 'YES' else 'NO')")
        else {
            return;
        };
        if available.output.trim() != "YES" {
            return;
        }
        let created = session.execute("graph_plot <- ggplot2::ggplot(mtcars, ggplot2::aes(wt, mpg, colour=factor(cyl))) + ggplot2::geom_point() + ggplot2::theme_classic() + ggplot2::labs(title='Fuel economy', colour='Cylinders')\ngraph_plot_before <- serialize(graph_plot, NULL)").unwrap();
        assert!(created.error.is_none(), "{:?}", created.error);
        let left =
            GraphStyle { legend_position: Some(LegendPosition::Left), plot_title_pt: Some(18.0), axis_x_line_mm: Some(1.2), ..Default::default() };
        let none = GraphStyle { legend_position: Some(LegendPosition::None), plot_title_pt: Some(9.0), ..Default::default() };
        let base = session.render_plot_svg_with_graph_style("graph_plot", 70.0, 50.0, true, &GraphStyle::default()).unwrap().0;
        let thicker_axis = session
            .render_plot_svg_with_graph_style("graph_plot", 70.0, 50.0, true, &GraphStyle { axis_x_line_mm: Some(1.2), ..Default::default() })
            .unwrap()
            .0;
        let larger_title = session
            .render_plot_svg_with_graph_style("graph_plot", 70.0, 50.0, true, &GraphStyle { plot_title_pt: Some(18.0), ..Default::default() })
            .unwrap()
            .0;
        let left_legend = session
            .render_plot_svg_with_graph_style(
                "graph_plot",
                70.0,
                50.0,
                true,
                &GraphStyle { legend_position: Some(LegendPosition::Left), ..Default::default() },
            )
            .unwrap()
            .0;
        assert_ne!(base, thicker_axis, "axis line width must change SVG geometry");
        assert_ne!(base, larger_title, "title point size must change SVG text");
        assert_ne!(base, left_legend, "legend position must change SVG layout");
        let first = session.render_plot_svg_with_graph_style("graph_plot", 70.0, 50.0, true, &left).unwrap().0;
        let second = session.render_plot_svg_with_graph_style("graph_plot", 100.0, 80.0, true, &none).unwrap().0;
        assert!(first.contains("<svg") && second.contains("<svg"));
        assert_ne!(first, second);
        assert!(first.contains("Cylinders"), "legend should render in the first panel");
        assert!(!second.contains("Cylinders"), "legend should be hidden in the second panel");
        let unchanged = session.execute("cat(identical(graph_plot_before, serialize(graph_plot, NULL)))").unwrap();
        assert_eq!(unchanged.output.trim(), "TRUE");
        assert!(session.stop().is_ok());
    }

    #[test]
    fn advanced_graph_theme_controls_change_svg_without_mutating_the_named_plot() {
        let mut session = RSession::default();
        let Ok(available) =
            session.execute("cat(if (requireNamespace('ggplot2', quietly=TRUE) && requireNamespace('svglite', quietly=TRUE)) 'YES' else 'NO')")
        else {
            return;
        };
        if available.output.trim() != "YES" {
            return;
        }
        let created = session
            .execute(
                "advanced_plot <- ggplot2::ggplot(mtcars, ggplot2::aes(wt, mpg, colour=factor(gear))) + ggplot2::geom_point() + ggplot2::scale_x_continuous(breaks=c(2,4), labels=c('TICK_ALPHA','TICK_BETA')) + ggplot2::facet_wrap(~cyl) + ggplot2::labs(title='ADVANCED_TITLE', subtitle='ADVANCED_SUBTITLE', caption='ADVANCED_CAPTION', x='WEIGHT_LABEL', colour='GEARS') + ggplot2::theme_minimal()\nadvanced_plot_before <- serialize(advanced_plot, NULL)",
            )
            .unwrap();
        assert!(created.error.is_none(), "{:?}", created.error);
        let render = |session: &mut RSession, style: &GraphStyle| {
            session.render_plot_svg_with_graph_style("advanced_plot", 100.0, 80.0, false, style).unwrap().0
        };
        let base = render(&mut session, &GraphStyle::default());
        let typography = GraphStyle {
            title_details: TextDetails {
                family: Some("sans".into()),
                face: Some(FontFace::BoldItalic),
                colour: Some("#A12455".into()),
                ..Default::default()
            },
            tick_x_details: TextDetails { angle: Some(45.0), hjust: Some(1.0), vjust: Some(0.0), ..Default::default() },
            subtitle_pt: Some(15.0),
            caption_visible: Some(false),
            ..Default::default()
        };
        let text_svg = render(&mut session, &typography);
        assert_ne!(base, text_svg);
        assert!(text_svg.contains("ADVANCED_TITLE") && text_svg.contains("ADVANCED_SUBTITLE"));
        assert!(!text_svg.contains("ADVANCED_CAPTION"));
        assert!(text_svg.to_ascii_lowercase().contains("#a12455"));
        let panel = GraphStyle {
            major_grid_x_visible: Some(false),
            major_grid_y_visible: Some(true),
            minor_grid_x_visible: Some(true),
            minor_grid_y_visible: Some(false),
            major_grid_colour: Some("#1278AB".into()),
            minor_grid_colour: Some("tomato".into()),
            panel_background: Some("#FDF0C1".into()),
            plot_background: Some("#F8F8F8".into()),
            panel_border_visible: Some(true),
            panel_border_colour: Some("#AA0033".into()),
            panel_border_width_mm: Some(0.5),
            ..Default::default()
        };
        let panel_svg = render(&mut session, &panel);
        assert_ne!(base, panel_svg);
        let grid_x_svg = render(&mut session, &GraphStyle { major_grid_x_visible: Some(false), ..Default::default() });
        let grid_y_svg = render(&mut session, &GraphStyle { major_grid_y_visible: Some(false), ..Default::default() });
        assert_ne!(grid_x_svg, grid_y_svg, "X and Y grid visibility must affect different SVG geometry");
        let minor_x_svg = render(&mut session, &GraphStyle { minor_grid_x_visible: Some(false), ..Default::default() });
        let minor_y_svg = render(&mut session, &GraphStyle { minor_grid_y_visible: Some(false), ..Default::default() });
        assert_ne!(minor_x_svg, minor_y_svg, "minor X and Y grids must remain independent");
        for colour in ["#fdf0c1", "#aa0033", "#1278ab"] {
            assert!(panel_svg.to_ascii_lowercase().contains(colour), "{colour} missing from SVG");
        }
        let mut ticks = GraphStyle {
            axis_x_ticks_visible: Some(false),
            axis_y_ticks_visible: Some(true),
            axis_x_tick_colour: Some("#004499".into()),
            axis_y_tick_length_mm: Some(3.0),
            ..Default::default()
        };
        let no_x_ticks = render(&mut session, &ticks);
        assert!(no_x_ticks.contains("TICK_ALPHA") && no_x_ticks.contains("WEIGHT_LABEL"), "tick mark visibility must not hide tick or axis text");
        ticks.axis_x_ticks_visible = Some(true);
        let with_x_ticks = render(&mut session, &ticks);
        assert_ne!(no_x_ticks, with_x_ticks);
        let legend = GraphStyle {
            legend_position: Some(LegendPosition::Bottom),
            legend_direction: Some(LegendDirection::Horizontal),
            legend_justification: Some([0.0, 1.0]),
            legend_key_width_mm: Some(8.0),
            legend_key_height_mm: Some(4.0),
            legend_key_background: Some("#CCEEAA".into()),
            legend_background: Some("#EEF6FF".into()),
            legend_spacing_x_mm: Some(4.0),
            legend_margin_mm: Some([1.0, 2.0, 3.0, 4.0]),
            plot_margin_mm: Some([5.0, 6.0, 7.0, 8.0]),
            strip_text_details: TextDetails { face: Some(FontFace::Bold), ..Default::default() },
            strip_background: Some("#DDEEFF".into()),
            panel_spacing_mm: Some(3.0),
            ..Default::default()
        };
        let legend_svg = render(&mut session, &legend);
        assert_ne!(base, legend_svg);
        assert!(legend_svg.contains("GEARS"));
        assert!(legend_svg.to_ascii_lowercase().contains("#ddeeff"));
        let invalid_colour = session.render_plot_svg_with_graph_style(
            "advanced_plot",
            100.0,
            80.0,
            false,
            &GraphStyle { plot_background: Some("notacolour".into()), ..Default::default() },
        );
        assert!(invalid_colour.is_err_and(|error| error.contains("Invalid R colour")));
        let unchanged = session.execute("cat(identical(advanced_plot_before, serialize(advanced_plot, NULL)))").unwrap();
        assert_eq!(unchanged.output.trim(), "TRUE");
    }

    #[test]
    fn graph_text_visibility_uses_element_blank_without_changing_the_r_object() {
        let mut session = RSession::default();
        let Ok(available) =
            session.execute("cat(if (requireNamespace('ggplot2', quietly=TRUE) && requireNamespace('svglite', quietly=TRUE)) 'YES' else 'NO')")
        else {
            return;
        };
        if available.output.trim() != "YES" {
            return;
        }
        let setup = session.execute("text_plot <- ggplot2::ggplot(mtcars, ggplot2::aes(wt, mpg)) + ggplot2::geom_point() + ggplot2::scale_x_continuous(breaks=c(2,4), labels=c('X_TICK_A','X_TICK_B')) + ggplot2::labs(title='PLOT_TITLE', x='X_AXIS_TITLE', y='Y_AXIS_TITLE') + ggplot2::theme_classic()\ntext_plot_before <- serialize(text_plot, NULL)\noriginally_hidden <- text_plot + ggplot2::theme(axis.text.x=ggplot2::element_blank())").unwrap();
        assert!(setup.error.is_none(), "{:?}", setup.error);
        let render = |session: &mut RSession, name: &str, style: GraphStyle| {
            session.render_plot_svg_with_graph_style(name, 70.0, 50.0, false, &style).unwrap().0
        };
        let base = render(&mut session, "text_plot", GraphStyle::default());
        assert!(base.contains("X_TICK_A") && base.contains("X_AXIS_TITLE") && base.contains("PLOT_TITLE"));
        let no_ticks = render(&mut session, "text_plot", GraphStyle { tick_x_visible: Some(false), ..Default::default() });
        assert!(!no_ticks.contains("X_TICK_A"));
        assert!(no_ticks.contains("X_AXIS_TITLE") && no_ticks.contains("PLOT_TITLE"));
        let no_x_title = render(&mut session, "text_plot", GraphStyle { axis_title_x_visible: Some(false), ..Default::default() });
        assert!(!no_x_title.contains("X_AXIS_TITLE"));
        assert!(no_x_title.contains("X_TICK_A") && no_x_title.contains("PLOT_TITLE"));
        let no_plot_title = render(&mut session, "text_plot", GraphStyle { plot_title_visible: Some(false), ..Default::default() });
        assert!(!no_plot_title.contains("PLOT_TITLE"));
        assert!(no_plot_title.contains("X_TICK_A") && no_plot_title.contains("X_AXIS_TITLE"));
        let inherited = render(&mut session, "originally_hidden", GraphStyle::default());
        assert!(!inherited.contains("X_TICK_A"));
        let explicit_show =
            render(&mut session, "originally_hidden", GraphStyle { tick_x_visible: Some(true), tick_x_pt: Some(18.0), ..Default::default() });
        assert!(explicit_show.contains("X_TICK_A"));
        let unchanged = session.execute("cat(identical(text_plot_before, serialize(text_plot, NULL)))").unwrap();
        assert_eq!(unchanged.output.trim(), "TRUE");
        assert!(session.stop().is_ok());
    }

    #[test]
    fn named_pheatmap_objects_render_grid_svg_and_ordinary_lists_are_rejected() {
        let mut session = RSession::default();
        let available = session.execute("cat(if (requireNamespace('pheatmap', quietly=TRUE)) 'YES' else 'NO')").unwrap();
        if available.output.trim() != "YES" {
            return;
        }
        let setup = session
            .execute(
                "mat <- matrix(seq(-2, 2, length.out=100), nrow=10, dimnames=list(paste0('long_gene_label_', 1:10), paste0('long_sample_label_', 1:10)))\nannotation <- data.frame(Group=rep(c('A','B'), each=5), row.names=colnames(mat))\ncolors <- grDevices::colorRampPalette(c('#0000FF','#FFFFFF','#FF0000'))(20)\nheatmap_silent <- pheatmap::pheatmap(mat, silent=TRUE, color=colors, annotation_col=annotation)\ntemp_pdf <- tempfile(fileext='.pdf'); grDevices::pdf(file=temp_pdf)\nheatmap_drawn <- pheatmap::pheatmap(mat, color=colors, annotation_col=annotation)\ngrDevices::dev.off(); unlink(temp_pdf); rm(temp_pdf)\nordinary <- list(gtable=1)\ninvisible(pheatmap::pheatmap(mat, silent=TRUE))",
            )
            .unwrap();
        assert!(setup.error.is_none(), "{:?}", setup.error);
        for name in ["heatmap_silent", "heatmap_drawn"] {
            let plot = setup.objects.iter().find(|object| object.variable == name).unwrap();
            assert_eq!(plot.plot_kind, "pheatmap");
            assert_eq!(plot.category, "Values");
            assert_eq!(plot.description, "pheatmap::pheatmap");
        }
        assert_eq!(setup.objects.iter().find(|object| object.variable == "ordinary").unwrap().plot_kind, "");
        assert_eq!(setup.objects.iter().filter(|object| object.plot_kind == "pheatmap").count(), 2);
        let svg = session.render_plot_svg_at_size("heatmap_silent", 140.0, 100.0).unwrap().0;
        let wide = session.render_plot_svg_at_size("heatmap_drawn", 180.0, 100.0).unwrap().0;
        let artifacts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/r-workflow-pheatmap");
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::write(artifacts.join("source.svg"), &svg).unwrap();
        std::fs::write(artifacts.join("wide.svg"), &wide).unwrap();
        assert!(svg.contains("<svg") && svg.matches("<path").count() + svg.matches("<rect").count() > 10);
        let svg_lower = svg.to_ascii_lowercase();
        let blue = svg.contains("rgb(0%, 0%, 100%)") || svg_lower.contains("#0000ff");
        let red = svg.contains("rgb(100%, 0%, 0%)") || svg_lower.contains("#ff0000");
        assert!(blue && red, "heatmap source colors missing");
        assert!((svg_root_dimension_mm(&svg, "width").unwrap() - 140.0).abs() < 0.5);
        assert!((svg_root_dimension_mm(&wide, "width").unwrap() - 180.0).abs() < 0.5);
        let mut document = vectorcraft_engine::Session::new();
        document.execute("file.new", &serde_json::json!({"width": 600, "height": 400})).unwrap();
        let imported = document.execute("clipboard.importSvg", &serde_json::json!({"svg": svg})).unwrap();
        assert!(imported["count"].as_u64().is_some_and(|count| count > 0));
        document.execute("edit.pasteInPlace", &serde_json::json!({})).unwrap();
        let exported = document.execute("document.serialize", &serde_json::json!({"format":"svg"})).unwrap();
        let exported_svg = exported["text"].as_str().unwrap();
        std::fs::write(artifacts.join("imported-export.svg"), exported_svg).unwrap();
        assert!(
            exported_svg.to_ascii_lowercase().contains("#0000ff") && exported_svg.to_ascii_lowercase().contains("#ff0000"),
            "heatmap imported colors missing"
        );
        assert!(session.render_plot_svg_at_size("ordinary", 140.0, 100.0).is_err());
        let unsupported = session.render_plot_svg_with_graph_style(
            "heatmap_silent",
            140.0,
            100.0,
            false,
            &GraphStyle { plot_background: Some("#ffffff".into()), ..Default::default() },
        );
        assert!(unsupported.is_err_and(|error| error.contains("ggplot")), "pheatmap must not receive ggplot theme controls");
        assert!(session.stop().is_ok());
    }

    #[test]
    fn ggplot_gradient2_colors_survive_r_svg_import_and_export() {
        let mut session = RSession::default();
        let available = session.execute("cat(if (requireNamespace('ggplot2', quietly=TRUE)) 'YES' else 'NO')").unwrap();
        if available.output.trim() != "YES" {
            return;
        }
        let source = "df <- data.frame(CellLine=c('A','B','C','D'), Description=c('first','second','third','fourth'), setSize=c(2,4,6,8), NES=c(-2,-1,1,2))\ndot_plot <- ggplot2::ggplot(df, ggplot2::aes(x=CellLine, y=Description, size=setSize, color=NES)) + ggplot2::geom_point() + ggplot2::scale_color_gradient2(low='blue', mid='white', high='red', midpoint=0) + ggplot2::scale_size_continuous() + ggplot2::theme_bw() + ggplot2::labs(color='NES\\nscore') + ggplot2::ggtitle('Synthetic dot plot')";
        let result = session.execute(source).unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        let built = session.execute("cat(paste(unique(ggplot2::ggplot_build(dot_plot)$data[[1L]]$colour), collapse=','))").unwrap();
        assert!(built.error.is_none(), "{:?}", built.error);
        let colors: Vec<_> = built.output.trim().split(',').map(str::to_ascii_lowercase).collect();
        assert!(colors.len() >= 3 && colors.iter().any(|color| color != "#000000"), "built colors: {:?}", colors);
        let (svg, _) = session.render_plot_svg_at_size("dot_plot", 160.0, 110.0).unwrap();
        let artifacts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/r-workflow-color");
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::write(artifacts.join("source-r.svg"), &svg).unwrap();
        std::fs::write(artifacts.join("built-colors.txt"), colors.join("\n")).unwrap();
        let mut document = vectorcraft_engine::Session::new();
        document.execute("file.new", &serde_json::json!({"width": 650, "height": 450})).unwrap();
        let imported = document.execute("clipboard.importSvg", &serde_json::json!({"svg": svg})).unwrap();
        assert!(imported["count"].as_u64().is_some_and(|count| count > 0), "imported: {imported}");
        document.execute("edit.pasteInPlace", &serde_json::json!({})).unwrap();
        let exported = document.execute("document.serialize", &serde_json::json!({"format": "svg"})).unwrap();
        let exported_svg = exported["text"].as_str().unwrap();
        std::fs::write(artifacts.join("imported-export.svg"), exported_svg).unwrap();
        let rendered = vectorcraft_render::Renderer::new().render(
            &document.active().unwrap().doc,
            650,
            450,
            vectorcraft_geom::Affine::IDENTITY,
            &vectorcraft_render::RenderOptions::default(),
        );
        let (pixels, remainder) = rendered.pixels.as_chunks::<4>();
        assert!(remainder.is_empty());
        let white_area = pixels.iter().filter(|pixel| pixel[0] > 235 && pixel[1] > 235 && pixel[2] > 235 && pixel[3] > 200).count();
        let blue_pixels = pixels.iter().filter(|pixel| pixel[2] > pixel[0].saturating_add(35) && pixel[2] > pixel[1].saturating_add(35)).count();
        let red_pixels = pixels.iter().filter(|pixel| pixel[0] > pixel[1].saturating_add(35) && pixel[0] > pixel[2].saturating_add(35)).count();
        assert!(
            white_area > 50_000 && blue_pixels > 10 && red_pixels > 10,
            "rendered plot has no visible color: white={white_area} blue={blue_pixels} red={red_pixels}"
        );
        image::RgbaImage::from_raw(650, 450, rendered.pixels).unwrap().save(artifacts.join("imported-render.png")).unwrap();
        let source_svg = svg.to_ascii_lowercase();
        let export_svg = exported_svg.to_ascii_lowercase();
        let exported_matches = colors.iter().filter(|color| export_svg.contains(color.as_str())).count();
        let source_matches = colors.iter().filter(|color| source_svg.contains(color.as_str())).count();
        assert!(source_matches >= 3, "R SVG lost ggplot colors: {colors:?}; found {source_matches}");
        assert!(exported_matches >= 3, "VectorCraft SVG lost ggplot colors: {colors:?}; found {exported_matches}");
        assert!(session.stop().is_ok());
    }

    #[test]
    fn ggplot_reflow_uses_svg_device_dimensions_in_millimeters() {
        let mut session = RSession::default();
        let setup = session.execute("if (!requireNamespace('ggplot2', quietly=TRUE)) stop('missing ggplot2')\nplot_A <- ggplot2::ggplot(mtcars, ggplot2::aes(wt, mpg)) + ggplot2::geom_point(size=3) + ggplot2::labs(title='Weight vs MPG', x='Weight', y='MPG') + ggplot2::theme_classic(base_size=10)").unwrap();
        if setup.error.is_some() {
            return;
        }
        let first = session.render_plot_svg_at_size("plot_A", 100.0, 80.0).unwrap().0;
        let wide = session.render_plot_svg_at_size("plot_A", 160.0, 80.0).unwrap().0;
        let tall = session.render_plot_svg_at_size("plot_A", 100.0, 120.0).unwrap().0;
        // SVG devices write intrinsic dimensions in points, with either quote style.
        assert!((svg_root_dimension_mm(&first, "width").unwrap() - 100.0).abs() < 0.25);
        assert!((svg_root_dimension_mm(&first, "height").unwrap() - 80.0).abs() < 0.5);
        assert!((svg_root_dimension_mm(&wide, "width").unwrap() - 160.0).abs() < 0.25);
        assert!((svg_root_dimension_mm(&wide, "height").unwrap() - 80.0).abs() < 0.5);
        assert!((svg_root_dimension_mm(&tall, "width").unwrap() - 100.0).abs() < 0.25);
        assert!((svg_root_dimension_mm(&tall, "height").unwrap() - 120.0).abs() < 0.5);
        assert_ne!(first, wide);
        assert_ne!(first, tall);
        assert!(first.starts_with("<?xml") && first.contains("<svg"));
        let vector_elements = |svg: &str| svg.matches("<path").count() + svg.matches("<circle").count() + svg.matches("<text").count();
        assert!(vector_elements(&first) > 10, "R SVG should contain editable vector geometry and text");
        assert!(vector_elements(&wide) > 10 && vector_elements(&tall) > 10);
        assert!(!first.contains("plot_A <-"));
        assert!(setup.objects.iter().any(|object| object.variable == "plot_A" && object.classes.contains(&"ggplot".to_owned())));
        assert!(session.stop().is_ok());
    }

    fn svg_root_dimension_mm(svg: &str, attribute: &str) -> Option<f64> {
        let root_start = svg.find("<svg")?;
        let root_end = svg.get(root_start..)?.find('>')?.saturating_add(root_start);
        let root = svg.get(root_start..root_end)?;
        let double_marker = format!("{attribute}=\"");
        let single_marker = format!("{attribute}='");
        let value = root
            .split_once(&double_marker)
            .and_then(|(_, rest)| rest.split_once('"').map(|(value, _)| value))
            .or_else(|| root.split_once(&single_marker).and_then(|(_, rest)| rest.split_once('\'').map(|(value, _)| value)))?;
        let number_end = value.find(|character: char| !character.is_ascii_digit() && character != '.' && character != '-')?;
        let number = value.get(..number_end)?.parse::<f64>().ok()?;
        let unit = value.get(number_end..)?;
        match unit {
            "pt" => Some(number * 25.4 / 72.0),
            "mm" => Some(number),
            "in" => Some(number * 25.4),
            "px" => Some(number * 25.4 / 96.0),
            _ => None,
        }
    }

    #[test]
    fn unsupported_plot_objects_are_rejected_and_temporary_svg_is_cleaned() {
        if super::find_rscript().is_err() {
            return;
        }
        let mut session = RSession::default();
        session.execute("unsupported_plot <- list(value = 1)").unwrap();
        let process = session.process.as_ref().unwrap();
        let path = process.request_path.with_file_name("plot.svg");
        let result = session.render_plot_svg("unsupported_plot");
        assert!(result.is_err());
        assert!(!path.exists(), "failed rendering must remove its temporary SVG");
    }

    #[test]
    fn failed_ggplot_render_closes_its_graphics_device() {
        if super::find_rscript().is_err() {
            return;
        }
        let mut session = RSession::default();
        let setup = session
            .execute("broken_plot <- structure(list(), class = 'ggplot')\nprint.ggplot <- function(x, ...) stop('intentional render failure')")
            .unwrap();
        assert!(setup.error.is_none(), "{:?}", setup.error);
        let error = session.render_plot_svg_at_size("broken_plot", 100.0, 80.0).unwrap_err();
        assert!(error.contains("intentional render failure"), "{error}");
        let devices = session.execute("grDevices::dev.list()").unwrap();
        assert_eq!(devices.output.trim(), "NULL", "failed plot render leaked a graphics device");
        let _ = session.execute("rm(print.ggplot)");
        assert!(session.stop().is_ok());
    }

    #[test]
    fn package_inventory_and_text_help_share_the_persistent_session() {
        if super::find_rscript().is_err() {
            return;
        }
        let mut session = RSession::default();
        let packages = session.packages().unwrap();
        assert!(packages.iter().any(|package| package.name == "base" && package.loaded));
        let help = session.help_text("mean").unwrap();
        assert!(help.to_lowercase().contains("arithmetic") || help.to_lowercase().contains("mean"));
        let value = session.execute("6 * 7").unwrap();
        assert!(value.output.contains("42"));
        assert!(session.stop().is_ok());
    }

    #[test]
    fn parse_rdata_inspection_decodes_names_and_collisions() {
        let hex_df = super::encode_hex(b"df");
        let hex_p1 = super::encode_hex(b"p1");
        let raw = format!("2\t1\t{hex_df},{hex_p1}\t{hex_df}");
        let inspection = super::parse_rdata_inspection("test.RData", &raw).unwrap();
        assert_eq!(inspection.file_path, "test.RData");
        assert_eq!(inspection.total_objects, 2);
        assert_eq!(inspection.object_names, vec!["df", "p1"]);
        assert_eq!(inspection.collisions, vec!["df"]);
    }

    #[test]
    fn parse_rdata_inspection_handles_empty_set() {
        let raw = "0\t0\t\t";
        let inspection = super::parse_rdata_inspection("empty.rda", raw).unwrap();
        assert_eq!(inspection.total_objects, 0);
        assert!(inspection.object_names.is_empty());
        assert!(inspection.collisions.is_empty());
    }

    #[test]
    fn parse_apply_rdata_output_parses_counts() {
        let raw = "5\t2";
        let (imported, skipped) = super::parse_apply_rdata_output(raw).unwrap();
        assert_eq!(imported, 5);
        assert_eq!(skipped, 2);
    }

    #[test]
    fn rdata_inspection_and_import_with_session() {
        if super::find_rscript().is_err() {
            return;
        }
        let mut session = RSession::default();
        let temp_dir = std::env::temp_dir().join(format!("vcstudio-test-rdata-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let rdata_path = temp_dir.join("test_import.RData");
        let rdata_str = rdata_path.to_str().unwrap().replace('\\', "/");

        let setup = session
            .execute(&format!(
                "local({{\n  my_ggplot <- structure(list(), class = 'ggplot')\n  plot_table <- data.frame(a = 1:5)\n  existing_var <- 10\n  save(my_ggplot, plot_table, existing_var, file = '{rdata_str}')\n}})"
            ))
            .unwrap();
        assert!(setup.error.is_none(), "{:?}", setup.error);

        let prep = session.execute("existing_var <- 999\nrm(list = setdiff(ls(), 'existing_var'))").unwrap();
        assert!(prep.error.is_none());

        let inspection = session.inspect_rdata(&rdata_str).unwrap();
        assert_eq!(inspection.total_objects, 3);
        assert!(inspection.object_names.contains(&"my_ggplot".into()));
        assert!(inspection.object_names.contains(&"plot_table".into()));
        assert!(inspection.object_names.contains(&"existing_var".into()));
        assert_eq!(inspection.collisions, vec!["existing_var"]);

        let (exec_skip, imported, skipped) = session.apply_rdata_import(&rdata_str, super::ImportCollisionPolicy::Skip).unwrap();
        assert_eq!(imported, 2);
        assert_eq!(skipped, 1);
        let val_check = session.execute("existing_var").unwrap();
        assert!(val_check.output.contains("999"), "Skip policy must preserve existing colliding value");

        let ggplot_obj = exec_skip.objects.iter().find(|o| o.variable == "my_ggplot").unwrap();
        assert_eq!(ggplot_obj.plot_kind, "ggplot");

        let non_plot_obj = exec_skip.objects.iter().find(|o| o.variable == "plot_table").unwrap();
        assert_eq!(non_plot_obj.plot_kind, "", "Objects with 'plot' in name that are not plots must have empty plot_kind");

        let (_exec_overwrite, imported_ow, skipped_ow) = session.apply_rdata_import(&rdata_str, super::ImportCollisionPolicy::Overwrite).unwrap();
        assert_eq!(imported_ow, 3);
        assert_eq!(skipped_ow, 0);
        let val_check2 = session.execute("existing_var").unwrap();
        assert!(val_check2.output.contains("10"), "Overwrite policy must overwrite colliding value");

        let _ = std::fs::remove_file(&rdata_path);
        let _ = std::fs::remove_dir(&temp_dir);
        assert!(session.stop().is_ok());
    }

    #[test]
    fn rdata_invalid_file_returns_error() {
        if super::find_rscript().is_err() {
            return;
        }
        let mut session = RSession::default();
        let temp_dir = std::env::temp_dir().join(format!("vcstudio-test-corrupt-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let corrupt_path = temp_dir.join("corrupt.RData");
        std::fs::write(&corrupt_path, b"not an rdata file content").unwrap();

        let inspect_res = session.inspect_rdata(corrupt_path.to_str().unwrap());
        assert!(inspect_res.is_err(), "Corrupted RData file must return an error");
        let error_msg = inspect_res.unwrap_err();
        assert!(error_msg.contains("Failed to load RData file") || error_msg.contains("magic number") || error_msg.contains("error"));

        let _ = std::fs::remove_file(&corrupt_path);
        let _ = std::fs::remove_dir(&temp_dir);
        assert!(session.stop().is_ok());
    }
}
