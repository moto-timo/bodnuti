**check:** An empty shell function in a recipe (`do_compile () {` then `}`, as rust's recipe has) no longer makes shellcheck fail to parse it and leave the recipe's shell unchecked.
