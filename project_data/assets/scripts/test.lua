function on_setup()
  log_bar("this works!!")
end

function on_update()
  log_bar("im sure updating")
end

return {
  on_setup = on_setup,
  on_update = on_update
}
